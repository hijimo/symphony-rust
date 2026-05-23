# Symphony Web Platform API - Phase 4: 协作与控制 (Collaboration & Control)

## 概述

Phase 4 实现多用户协作场景下的 Token 隔离、全局并行控制、作者标识与筛选功能。核心目标：

1. **多用户 Token 隔离** — 看板使用当前用户 Token 访问平台 API（Phase 3 已实现），服务进程使用项目 Owner Token 运行
2. **全局并行控制** — 汇总所有运行中 Symphony 实例的活跃 Agent 数，达到上限时暂停调度
3. **作者标识与筛选** — Issue/PR 归属识别，支持按作者筛选看板内容
4. **前端监控面板** — 实时并行数监控、作者筛选 UI

## 通用协议

继承 Phase 1-3 所有通用协议，包括统一响应格式、错误码、认证方式、字段命名约定。

### 统一响应格式

```json
{ "data": T, "success": true, "retCode": "0", "retMsg": "ok" }
```

错误响应：
```json
{ "data": null, "success": false, "retCode": "CODE", "retMsg": "描述", "showType": N }
```

### 新增错误码

| retCode | 含义 | showType | HTTP Status |
|---------|------|----------|-------------|
| `CONCURRENCY_001` | 已达全局并行上限 | 1 (warn) | 429 |
| `CONCURRENCY_002` | 已达项目并行上限 | 1 (warn) | 429 |
| `TOKEN_002` | 项目 Owner Token 未配置或无效 | 1 (warn) | 400 |

**完整 TOKEN 错误码命名空间**（含已有）:

| retCode | 含义 | 使用场景 |
|---------|------|----------|
| `TOKEN_001` | 用户平台 Token 无效或过期（已有） | 看板/Issue 操作时用户自身 Token 失效 |
| `TOKEN_002` | 项目 Owner Token 未配置或无效（新增） | 启动服务时 Owner Token 校验失败 |

> 注意：`EXT_002` 为已有错误码（速率限制），Phase 4 复用该码用于平台 API 限流场景。

### 速率限制（Phase 4 新增端点）

| 端点 | 限制 | 说明 |
|------|------|------|
| `GET /api/admin/concurrency` | 60 次/分钟/用户 | 并行状态查询 |
| `PUT /api/admin/concurrency/config` | 10 次/分钟/用户 | 配置变更 |
| `GET /api/projects/:id/concurrency` | 60 次/分钟/用户 | 项目并行查询 |
| `GET /api/projects/:id/contributors` | 30 次/分钟/用户 | 贡献者列表 |
| `POST /api/user/config/validate-token` | 3 次/分钟/用户 | Token 验证（防枚举攻击） |

---

## 1. 多用户 Token 隔离

### 1.1 架构说明

Token 使用策略：

| 场景 | 使用的 Token | 说明 |
|------|-------------|------|
| 看板数据获取 | 当前登录用户的 Token | 用户只能看到自己 Token 权限范围内的数据 |
| Issue 创建 | 当前登录用户的 Token | Issue 以用户身份创建 |
| AI Issue 生成 | 当前登录用户的 Token | 同上 |
| Symphony 服务进程 | 项目 Owner 的 Token | 服务以 Owner 身份操作仓库 |
| 服务启动前校验 | 项目 Owner 的 Token | 启动前验证 Owner Token 有效性 |

### 1.2 Token 验证端点

#### POST /api/user/config/validate-token

验证用户配置的平台 Token 是否有效（调用平台 API 检查权限）。

**安全约束**:
- 此端点仅用于验证用户**即将保存到自己配置中**的 Token
- 不可用于验证他人的 Token（服务端不记录传入的 token 明文到日志）
- 速率限制：3 次/分钟/用户（防止 Token 枚举攻击）
- 响应时间标准化：无论 Token 有效与否，响应时间不低于 500ms（防止时序攻击）

**认证**: Bearer Token (JWT)

**请求体**:
```json
{
  "platform": "gitlab",
  "token": "glpat-xxxxxxxxxxxx",
  "host": "https://gitlab.example.com"
}
```

**请求字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| platform | string | 是 | `"gitlab"` 或 `"github"` |
| token | string | 是 | 待验证的平台 Token |
| host | string | 否 | 自定义 GitLab 主机地址（GitHub 忽略此字段） |

**成功响应** (200):
```json
{
  "data": {
    "valid": true,
    "username": "john_doe",
    "scopes": ["api", "read_repository", "write_repository"],
    "expires_at": "2025-12-31T23:59:59Z"
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**Token 无效响应** (200, valid=false):
```json
{
  "data": {
    "valid": false,
    "username": null,
    "scopes": [],
    "expires_at": null,
    "error_detail": "Token has been revoked"
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**错误响应**:
- `BIZ_001` (400): platform 字段无效
- `EXT_001` (502): 平台 API 不可用
- `EXT_002` (429): 速率限制

**Rust 数据模型**:
```rust
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidateTokenRequest {
    pub platform: String,
    pub token: String,
    pub host: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidateTokenResponse {
    pub valid: bool,
    pub username: Option<String>,
    pub scopes: Vec<String>,
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_detail: Option<String>,
}
```

---

### 1.3 项目 Token 状态查询

#### GET /api/projects/:id/token-status

查看项目成员的 Token 配置状态（仅 Owner/Admin 可见完整列表，普通成员只能看到自己的状态）。

**认证**: Bearer Token (JWT)  
**权限**: 项目成员

**路径参数**:

| 参数 | 类型 | 说明 |
|------|------|------|
| id | i64 | 项目 ID |

**成功响应** (200, Owner/Admin 视角):
```json
{
  "data": {
    "owner_token_configured": true,
    "owner_token_valid": true,
    "owner_username": "project_owner",
    "members": [
      {
        "user_id": 1,
        "username": "alice",
        "has_token": true,
        "token_valid": null
      },
      {
        "user_id": 2,
        "username": "bob",
        "has_token": false,
        "token_valid": null
      }
    ]
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**成功响应** (200, 普通成员视角):
```json
{
  "data": {
    "owner_token_configured": true,
    "owner_token_valid": null,
    "owner_username": null,
    "members": [
      {
        "user_id": 2,
        "username": "bob",
        "has_token": false,
        "token_valid": null
      }
    ]
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| owner_token_configured | bool | 项目 Owner 是否已配置对应平台的 Token |
| owner_token_valid | bool/null | Owner Token 是否有效（仅 Owner/Admin 可见） |
| owner_username | string/null | Owner 的平台用户名（仅 Owner/Admin 可见） |
| members[].user_id | i64 | 成员用户 ID |
| members[].username | string | 成员用户名 |
| members[].has_token | bool | 该成员是否配置了对应平台的 Token |
| members[].token_valid | bool/null | 不主动验证，始终为 null（避免批量调用平台 API） |

**错误响应**:
- `AUTH_001` (401): 未认证
- `AUTH_002` (403): 非项目成员
- `BIZ_002` (404): 项目不存在

**Rust 数据模型**:
```rust
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTokenStatus {
    pub owner_token_configured: bool,
    pub owner_token_valid: Option<bool>,
    pub owner_username: Option<String>,
    pub members: Vec<MemberTokenStatus>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberTokenStatus {
    pub user_id: i64,
    pub username: String,
    pub has_token: bool,
    pub token_valid: Option<bool>,
}
```

---

### 1.4 服务启动时的 Token 校验增强

现有 `POST /api/projects/:id/start` 端点增加 Owner Token 校验逻辑：

**新增校验流程**（在现有权限检查之后）：
1. 查找项目 Owner（`project_members` 表中 role='owner' 的用户）
2. 获取 Owner 的 `user_configs` 中对应平台的 Token
3. 解密 Token 并调用平台 API 验证有效性
4. 若 Token 无效或未配置，返回 `TOKEN_002` 错误

**新增错误响应** (400):
```json
{
  "data": null,
  "success": false,
  "retCode": "TOKEN_002",
  "retMsg": "项目 Owner 未配置有效的 GitLab Token，无法启动服务",
  "showType": 1
}
```

---

## 2. 全局并行控制

### 2.1 架构说明

并行控制机制：

```
┌─────────────────────────────────────────────────────────┐
│                    Web Platform                          │
│                                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │         ConcurrencyManager (in-memory)           │   │
│  │                                                   │   │
│  │  global_max: 5 (from system_configs)             │   │
│  │  per_project_max: project.max_concurrent_agents  │   │
│  │                                                   │   │
│  │  active_agents: DashMap<project_id, AgentSlots>  │   │
│  │    project_1: { active: 2, max: 2 }             │   │
│  │    project_2: { active: 1, max: 3 }             │   │
│  │                                                   │   │
│  │  global_active() -> sum of all active = 3        │   │
│  └─────────────────────────────────────────────────┘   │
│                         │                               │
│                    polls every 5s                        │
│                         │                               │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐         │
│  │Symphony 1│    │Symphony 2│    │Symphony 3│         │
│  │(project1)│    │(project1)│    │(project2)│         │
│  │ PID=1234 │    │ PID=1235 │    │ PID=1236 │         │
│  └──────────┘    └──────────┘    └──────────┘         │
└─────────────────────────────────────────────────────────┘
```

**聚合机制**：
- Web Platform 维护一个 `ConcurrencyManager` 组件（内存中的 `DashMap`）
- 每个运行中的 Symphony 实例通过 stdout/log 文件报告当前活跃 Agent 数
- `ProcessManager` 的 watcher 线程每 5 秒轮询各实例状态文件（`/tmp/symphony-{project_id}-agents.json`）
- 状态文件由 Symphony 进程写入，格式：`{"active_agents": 2, "queued_tasks": 1, "updated_at": "..."}`
- 若状态文件超过 30 秒未更新，视为实例异常，active_agents 计为 0

**调度决策**：
- 启动新 Agent 前检查：`global_active + 1 <= global_max` AND `project_active + 1 <= project_max`
- 若超限，Symphony 实例暂停调度（不启动新 Agent），等待现有 Agent 完成
- Web Platform 不直接控制 Agent 调度，仅提供配置和监控；实际调度由 Symphony 进程根据配置自行决定

---

### 2.2 GET /api/admin/concurrency

获取全局并行状态概览。

**认证**: Bearer Token (JWT)  
**权限**: Admin

**成功响应** (200):
```json
{
  "data": {
    "global_max": 5,
    "global_active": 3,
    "global_queued": 2,
    "utilization_percent": 60.0,
    "projects": [
      {
        "project_id": 1,
        "project_name": "my-app",
        "max_agents": 2,
        "active_agents": 2,
        "queued_tasks": 1,
        "status": "running",
        "last_heartbeat": "2025-01-15T10:30:05Z"
      },
      {
        "project_id": 2,
        "project_name": "backend-service",
        "max_agents": 3,
        "active_agents": 1,
        "queued_tasks": 1,
        "status": "running",
        "last_heartbeat": "2025-01-15T10:30:03Z"
      }
    ],
    "throttled_projects": [1],
    "updated_at": "2025-01-15T10:30:05Z"
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| global_max | i64 | 全局最大并行 Agent 数（来自 system_configs） |
| global_active | i64 | 当前全局活跃 Agent 总数 |
| global_queued | i64 | 当前全局排队任务总数 |
| utilization_percent | f64 | 利用率 = global_active / global_max * 100 |
| projects[] | array | 各项目的并行状态 |
| projects[].project_id | i64 | 项目 ID |
| projects[].project_name | string | 项目名称 |
| projects[].max_agents | i64 | 项目最大并行数 |
| projects[].active_agents | i64 | 项目当前活跃 Agent 数 |
| projects[].queued_tasks | i64 | 项目排队任务数 |
| projects[].status | string | 服务状态（running/stopped/error） |
| projects[].last_heartbeat | string | 最后心跳时间（ISO 8601） |
| throttled_projects | Vec<i64> | 当前被限流的项目 ID 列表 |
| updated_at | string | 数据聚合时间 |
| data_freshness_seconds | i64 | 距离上次成功轮询的秒数（>10 时前端应提示数据可能过期） |

**错误响应**:
- `AUTH_001` (401): 未认证
- `AUTH_002` (403): 非 Admin

**Rust 数据模型**:
```rust
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GlobalConcurrencyStatus {
    pub global_max: i64,
    pub global_active: i64,
    pub global_queued: i64,
    pub utilization_percent: f64,
    pub projects: Vec<ProjectConcurrencyInfo>,
    pub throttled_projects: Vec<i64>,
    pub updated_at: String,
    /// Seconds since last successful watcher poll. Frontend should warn if > 10.
    pub data_freshness_seconds: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConcurrencyInfo {
    pub project_id: i64,
    pub project_name: String,
    pub max_agents: i64,
    pub active_agents: i64,
    pub queued_tasks: i64,
    pub status: String,
    pub last_heartbeat: Option<String>,
}
```

---

### 2.3 PUT /api/admin/concurrency/config

更新全局并行控制配置。

**认证**: Bearer Token (JWT)  
**权限**: Admin

**请求体**:
```json
{
  "global_max": 8,
  "expected_previous": 5
}
```

**请求字段说明**:

| 字段 | 类型 | 必填 | 约束 | 说明 |
|------|------|------|------|------|
| global_max | i64 | 是 | 1 <= x <= 50 | 全局最大并行 Agent 数 |
| expected_previous | i64 | 否 | - | 乐观锁：期望的当前值。若提供且不匹配实际值，返回 BIZ_003 |

**成功响应** (200):
```json
{
  "data": {
    "global_max": 8,
    "previous_value": 5,
    "effective_immediately": true
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**错误响应**:
- `AUTH_002` (403): 非 Admin
- `BIZ_001` (400): global_max 超出范围 [1, 50]
- `BIZ_003` (409): expected_previous 不匹配当前值（乐观锁冲突）

**Rust 数据模型**:
```rust
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConcurrencyConfigRequest {
    pub global_max: i64,
    /// Optimistic lock: if provided, the update will fail with BIZ_003
    /// if the current value doesn't match.
    pub expected_previous: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConcurrencyConfigResponse {
    pub global_max: i64,
    pub previous_value: i64,
    pub effective_immediately: bool,
}
```

**实现说明**:
- 更新 `system_configs` 表中 `max_concurrent_codex` 的值
- 同时更新内存中 `ConcurrencyManager` 的 `global_max`
- 变更立即生效：已运行的 Agent 不会被中断，但新的调度会遵循新限制
- 若新值小于当前 active 数，不会强制停止已运行的 Agent

---

### 2.4 GET /api/projects/:id/concurrency

获取单个项目的并行状态。

**认证**: Bearer Token (JWT)  
**权限**: 项目成员

**路径参数**:

| 参数 | 类型 | 说明 |
|------|------|------|
| id | i64 | 项目 ID |

**成功响应** (200):
```json
{
  "data": {
    "project_id": 1,
    "max_agents": 2,
    "active_agents": 2,
    "queued_tasks": 3,
    "is_throttled": true,
    "throttle_reason": "project_limit",
    "agents": [
      {
        "agent_id": "agent-abc123",
        "issue_iid": 42,
        "issue_title": "Implement user auth",
        "started_at": "2025-01-15T10:25:00Z",
        "elapsed_seconds": 305
      },
      {
        "agent_id": "agent-def456",
        "issue_iid": 43,
        "issue_title": "Fix pagination bug",
        "started_at": "2025-01-15T10:28:00Z",
        "elapsed_seconds": 125
      }
    ],
    "history": {
      "peak_today": 2,
      "total_tasks_today": 7,
      "avg_task_duration_seconds": 420
    },
    "last_heartbeat": "2025-01-15T10:30:05Z"
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| project_id | i64 | 项目 ID |
| max_agents | i64 | 项目最大并行数 |
| active_agents | i64 | 当前活跃 Agent 数 |
| queued_tasks | i64 | 排队等待的任务数 |
| is_throttled | bool | 是否正在被限流 |
| throttle_reason | string/null | 限流原因：`"project_limit"` / `"global_limit"` / null |
| agents[] | array | 当前活跃 Agent 详情 |
| agents[].agent_id | string | Agent 唯一标识 |
| agents[].issue_iid | u64 | 正在处理的 Issue IID |
| agents[].issue_title | string | Issue 标题 |
| agents[].started_at | string | Agent 启动时间 |
| agents[].elapsed_seconds | i64 | 已运行秒数 |
| history.peak_today | i64 | 今日峰值并行数 |
| history.total_tasks_today | i64 | 今日已完成任务数 |
| history.avg_task_duration_seconds | i64 | 平均任务耗时（秒） |
| last_heartbeat | string/null | 最后心跳时间 |

**错误响应**:
- `AUTH_002` (403): 非项目成员
- `BIZ_002` (404): 项目不存在

**Rust 数据模型**:
```rust
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConcurrencyDetail {
    pub project_id: i64,
    pub max_agents: i64,
    pub active_agents: i64,
    pub queued_tasks: i64,
    pub is_throttled: bool,
    pub throttle_reason: Option<String>,
    pub agents: Vec<ActiveAgentInfo>,
    pub history: ConcurrencyHistory,
    pub last_heartbeat: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAgentInfo {
    pub agent_id: String,
    pub issue_iid: u64,
    pub issue_title: String,
    pub started_at: String,
    pub elapsed_seconds: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConcurrencyHistory {
    pub peak_today: i64,
    pub total_tasks_today: i64,
    pub avg_task_duration_seconds: i64,
}
```

---

### 2.5 PUT /api/projects/:id/concurrency

更新项目级并行配置。

**认证**: Bearer Token (JWT)  
**权限**: 项目 Owner 或 Admin

**路径参数**:

| 参数 | 类型 | 说明 |
|------|------|------|
| id | i64 | 项目 ID |

**请求体**:
```json
{
  "max_agents": 3,
  "expected_previous": 2
}
```

**请求字段说明**:

| 字段 | 类型 | 必填 | 约束 | 说明 |
|------|------|------|------|------|
| max_agents | i64 | 是 | 1 <= x <= 10 | 项目最大并行 Agent 数 |
| expected_previous | i64 | 否 | - | 乐观锁：期望的当前值。若提供且不匹配，返回 BIZ_003 |

**成功响应** (200):
```json
{
  "data": {
    "project_id": 1,
    "max_agents": 3,
    "previous_value": 2,
    "effective_immediately": true
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**校验规则**:
- `max_agents` 不能超过 `global_max`（若超过返回 `BIZ_001`）
- 更新 `projects.max_concurrent_agents` 列
- 变更立即生效，已运行 Agent 不中断

**错误响应**:
- `AUTH_002` (403): 非 Owner/Admin
- `BIZ_001` (400): max_agents 超出范围或超过 global_max
- `BIZ_002` (404): 项目不存在
- `BIZ_003` (409): expected_previous 不匹配当前值（乐观锁冲突）

**Rust 数据模型**:
```rust
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectConcurrencyRequest {
    pub max_agents: i64,
    /// Optimistic lock: if provided, the update will fail with BIZ_003
    /// if the current value doesn't match.
    pub expected_previous: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectConcurrencyResponse {
    pub project_id: i64,
    pub max_agents: i64,
    pub previous_value: i64,
    pub effective_immediately: bool,
}
```

---

### 2.6 GET /api/admin/concurrency/events (SSE)

实时并行状态推送，使用 Server-Sent Events。

**认证**: 短期一次性 Ticket（通过 query parameter `ticket`）  
**权限**: Admin

**连接流程**:
1. 客户端先调用 `POST /api/admin/concurrency/events/ticket` 获取一次性 ticket（有效期 30 秒）
2. 客户端使用 ticket 连接 SSE：`GET /api/admin/concurrency/events?ticket=<one-time-ticket>`
3. 服务端验证 ticket 后立即作废（单次使用），建立 SSE 连接

> 安全说明：不使用 JWT 直接作为 query parameter，因为 URL 会被反向代理、浏览器历史、Referer 头等泄露。Ticket 为一次性短期凭证，泄露后无法重用。

#### POST /api/admin/concurrency/events/ticket

获取 SSE 连接用的一次性 ticket。

**认证**: Bearer Token (JWT)  
**权限**: Admin

**成功响应** (200):
```json
{
  "data": {
    "ticket": "sse-ticket-random-uuid-here",
    "expires_in_seconds": 30
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**请求**: `GET /api/admin/concurrency/events?ticket=<one-time-ticket>`

**事件类型**:

```
event: snapshot
data: {"global_active":3,"global_max":5,"projects":[...]}

event: agent_started
data: {"project_id":1,"agent_id":"agent-abc","issue_iid":42,"timestamp":"..."}

event: agent_completed
data: {"project_id":1,"agent_id":"agent-abc","issue_iid":42,"duration_seconds":300,"timestamp":"..."}

event: throttle_changed
data: {"project_id":1,"is_throttled":true,"reason":"global_limit","timestamp":"..."}

event: config_changed
data: {"global_max":8,"changed_by":"admin","timestamp":"..."}

event: heartbeat
data: {"timestamp":"..."}
```

**事件说明**:

| 事件 | 触发条件 | 频率 |
|------|----------|------|
| `snapshot` | 连接建立时 + 每 10 秒 | 10s 间隔 |
| `agent_started` | 新 Agent 启动 | 实时 |
| `agent_completed` | Agent 完成任务 | 实时 |
| `throttle_changed` | 限流状态变化 | 实时 |
| `config_changed` | 并行配置变更 | 实时 |
| `heartbeat` | 保活 | 30s 间隔 |

**连接管理**:
- 客户端断开后自动清理
- 服务端维护 `broadcast::channel` 用于事件分发
- 全局最大同时连接数：10（超出返回 503）
- 单用户最大同时连接数：2（超出返回 429，防止单用户耗尽所有连接槽）
- 连接超时：30 分钟无活动自动断开
- Ticket 验证失败返回 401 并关闭连接

**Rust 数据模型**:
```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConcurrencyEvent {
    Snapshot {
        global_active: i64,
        global_max: i64,
        global_queued: i64,
        projects: Vec<ProjectConcurrencyInfo>,
    },
    AgentStarted {
        project_id: i64,
        agent_id: String,
        issue_iid: u64,
        timestamp: String,
    },
    AgentCompleted {
        project_id: i64,
        agent_id: String,
        issue_iid: u64,
        duration_seconds: i64,
        timestamp: String,
    },
    ThrottleChanged {
        project_id: i64,
        is_throttled: bool,
        reason: Option<String>,
        timestamp: String,
    },
    ConfigChanged {
        global_max: i64,
        changed_by: String,
        timestamp: String,
    },
    Heartbeat {
        timestamp: String,
    },
}
```

---

## 3. 作者标识与筛选

### 3.1 架构说明

作者归属逻辑：

| 实体 | 作者判定 | 说明 |
|------|----------|------|
| Issue | `issue.author.username` | 平台 API 返回的创建者 |
| PR/MR (人工创建) | `mr.author.username` | 平台 API 返回的创建者 |
| PR/MR (Codex 创建) | 关联 Issue 的 author | 通过 `related_issue_iids` 追溯到原始 Issue 作者 |

**Codex PR 识别规则**：
- PR 的 `source_branch` 匹配模式 `symphony-*` 或 `codex-*`
- 或 PR 的 `author.username` 匹配 Symphony 服务使用的 bot 账号
- 满足任一条件时，PR 的"逻辑作者"为其关联 Issue 的 author

### 3.2 看板端点增强

#### GET /api/projects/:id/kanban (增强)

在现有 `KanbanQuery` 中新增 `author` 查询参数。

**新增查询参数**:

| 参数 | 类型 | 说明 |
|------|------|------|
| author | string | 按作者用户名筛选（平台用户名） |

**完整查询参数**（含已有参数）:

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| todo_limit | u32 | 50 | Todo 列最大返回数 (1-100) |
| assignee | string | - | 按 assignee 筛选 |
| author | string | - | 按作者筛选（新增） |
| labels | string | - | 按标签筛选（逗号分隔） |
| search | string | - | 按标题搜索 |
| no_cache | bool | false | 跳过缓存 |

**筛选行为**:
- `author` 参数筛选 Issue 的 `author.username` 字段
- 对于 PR 列：若 PR 是 Codex 创建的，按关联 Issue 的 author 筛选；否则按 PR 自身 author 筛选
- `author` 和 `assignee` 可同时使用（AND 关系）

**更新后的 Rust 模型**:
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct KanbanQuery {
    pub todo_limit: Option<u32>,
    pub assignee: Option<String>,
    pub author: Option<String>,  // 新增
    pub labels: Option<String>,
    pub search: Option<String>,
    pub no_cache: Option<bool>,
}
```

**实现注意**: 现有 `query_hash()` 函数必须同步更新，将 `query.author` 纳入哈希计算，否则不同 author 筛选会命中同一缓存条目。

**实现说明**:
- GitLab API 支持 `author_username` 参数，可直接传递
- GitHub API 支持 `creator` 参数，可直接传递
- PR 列的 author 筛选在服务端完成（获取全量后过滤）

---

### 3.3 GET /api/projects/:id/contributors

获取项目的所有贡献者列表（从 Issues 和 MRs 中聚合）。

**认证**: Bearer Token (JWT)  
**权限**: 项目成员

**路径参数**:

| 参数 | 类型 | 说明 |
|------|------|------|
| id | i64 | 项目 ID |

**查询参数**:

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| refresh | bool | false | 强制刷新（忽略缓存） |

**成功响应** (200):
```json
{
  "data": {
    "contributors": [
      {
        "username": "alice",
        "display_name": "Alice Wang",
        "avatar_url": "https://gitlab.com/uploads/-/system/user/avatar/123/avatar.png",
        "recent_issue_count": 15,
        "recent_mr_count": 8,
        "last_activity_at": "2025-01-15T09:00:00Z",
        "is_bot": false
      },
      {
        "username": "symphony-bot",
        "display_name": "Symphony Bot",
        "avatar_url": null,
        "recent_issue_count": 0,
        "recent_mr_count": 12,
        "last_activity_at": "2025-01-15T10:30:00Z",
        "is_bot": true
      }
    ],
    "total_count": 5,
    "scope": "last_100_items",
    "cached": true,
    "cached_at": "2025-01-15T10:25:00Z"
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| contributors[] | array | 贡献者列表，按 last_activity_at 降序 |
| contributors[].username | string | 平台用户名 |
| contributors[].display_name | string/null | 显示名称 |
| contributors[].avatar_url | string/null | 头像 URL |
| contributors[].recent_issue_count | u64 | 最近 100 条 Issue 中该用户创建的数量 |
| contributors[].recent_mr_count | u64 | 最近 100 条 MR 中该用户创建的数量 |
| contributors[].last_activity_at | string | 最后活动时间 |
| contributors[].is_bot | bool | 是否为 Bot 账号 |
| total_count | u64 | 贡献者总数 |
| scope | string | 数据范围说明（`"last_100_items"` 表示基于最近 100 条 Issue/MR 聚合） |
| cached | bool | 是否来自缓存 |
| cached_at | string/null | 缓存时间 |

**缓存策略**:
- 缓存 TTL：60 秒（贡献者列表变化不频繁）
- 缓存键：`{user_id}:{project_id}:contributors`
- `refresh=true` 时跳过缓存

**Bot 识别规则**:
- 优先匹配项目配置的 bot 账号列表（存储在 `projects` 表的扩展配置中）
- 回退启发式规则：用户类型为 `Bot`（GitLab API 返回 `bot: true`，GitHub API 返回 `type: "Bot"`）
- 最终回退：用户名包含 `[bot]` 后缀（GitHub App 惯例）
- 不使用简单子串匹配（如 "bot"），避免误判正常用户名（如 "robotics_engineer"）

**错误响应**:
- `AUTH_002` (403): 非项目成员
- `BIZ_002` (404): 项目不存在
- `TOKEN_001` (400): 用户 Token 无效
- `EXT_001` (502): 平台 API 不可用

**Rust 数据模型**:
```rust
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContributorsResponse {
    pub contributors: Vec<Contributor>,
    pub total_count: u64,
    pub scope: String,
    pub cached: bool,
    pub cached_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Contributor {
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub recent_issue_count: u64,
    pub recent_mr_count: u64,
    pub last_activity_at: String,
    pub is_bot: bool,
}
```

**实现说明**:
- 调用平台 API 获取最近 100 个 Issues 和 100 个 MRs 的 author 信息
- 在服务端聚合去重，统计每个 author 的 issue_count 和 mr_count
- 使用 singleflight 缓存避免重复请求

---

### 3.4 PR 归属追溯

#### GET /api/projects/:id/mrs/:iid (增强)

在现有 MR 详情响应中新增 `logical_author` 字段。

**新增响应字段**:

```json
{
  "data": {
    "iid": 15,
    "title": "Fix: resolve auth timeout issue",
    "author": {
      "username": "symphony-bot",
      "display_name": "Symphony Bot",
      "avatar_url": null
    },
    "logical_author": {
      "username": "alice",
      "display_name": "Alice Wang",
      "avatar_url": "https://...",
      "attribution_reason": "codex_created_pr"
    },
    "...": "其他现有字段不变"
  }
}
```

**新增字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| logical_author | object/null | 逻辑作者（仅当 PR 由 Bot 创建时填充） |
| logical_author.username | string | 逻辑作者的平台用户名 |
| logical_author.display_name | string/null | 显示名称 |
| logical_author.avatar_url | string/null | 头像 URL |
| logical_author.attribution_reason | string | 归属原因 |

**attribution_reason 枚举值**:

| 值 | 说明 |
|----|------|
| `codex_created_pr` | PR 由 Codex/Symphony 创建，归属到关联 Issue 作者 |
| `direct_author` | PR 由人工创建，author 即为逻辑作者（此时 logical_author 为 null） |

**Rust 数据模型**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogicalAuthor {
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub attribution_reason: String,
}
```

**更新 MergeRequestDetail**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MergeRequestDetail {
    // ... 现有字段 ...
    
    /// 逻辑作者（当 PR 由 Bot 创建时，追溯到关联 Issue 的作者）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_author: Option<LogicalAuthor>,
}
```

---

## 4. Admin Stats 增强

### 4.1 GET /api/admin/stats (增强)

在现有管理统计端点中增加并行控制相关数据。

**认证**: Bearer Token (JWT)  
**权限**: Admin

**成功响应** (200):
```json
{
  "data": {
    "users": {
      "total": 12,
      "active": 10,
      "admins": 2
    },
    "projects": {
      "total": 8,
      "running": 3,
      "stopped": 4,
      "error": 1
    },
    "concurrency": {
      "global_max": 5,
      "global_active": 3,
      "global_queued": 2,
      "utilization_percent": 60.0,
      "projects_at_limit": 1,
      "total_tasks_today": 15,
      "avg_task_duration_seconds": 380
    },
    "tokens": {
      "users_with_gitlab_token": 8,
      "users_with_github_token": 5,
      "users_without_any_token": 2
    }
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**新增字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| concurrency.global_max | i64 | 全局最大并行数 |
| concurrency.global_active | i64 | 当前活跃 Agent 总数 |
| concurrency.global_queued | i64 | 排队任务总数 |
| concurrency.utilization_percent | f64 | 利用率百分比 |
| concurrency.projects_at_limit | i64 | 达到并行上限的项目数 |
| concurrency.total_tasks_today | i64 | 今日完成任务总数 |
| concurrency.avg_task_duration_seconds | i64 | 平均任务耗时 |
| tokens.users_with_gitlab_token | i64 | 已配置 GitLab Token 的用户数 |
| tokens.users_with_github_token | i64 | 已配置 GitHub Token 的用户数 |
| tokens.users_without_any_token | i64 | 未配置任何 Token 的用户数 |

**Rust 数据模型**:
```rust
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminStats {
    pub users: UserStats,
    pub projects: ProjectStats,
    pub concurrency: ConcurrencyStats,
    pub tokens: TokenStats,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserStats {
    pub total: i64,
    pub active: i64,
    pub admins: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStats {
    pub total: i64,
    pub running: i64,
    pub stopped: i64,
    pub error: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConcurrencyStats {
    pub global_max: i64,
    pub global_active: i64,
    pub global_queued: i64,
    pub utilization_percent: f64,
    pub projects_at_limit: i64,
    pub total_tasks_today: i64,
    pub avg_task_duration_seconds: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TokenStats {
    pub users_with_gitlab_token: i64,
    pub users_with_github_token: i64,
    pub users_without_any_token: i64,
}
```

---

## 5. 数据库 Schema 变更

### 5.1 新增迁移文件: V003__phase4_concurrency.sql

> **注意**: 当前已有迁移为 V001 和 V002。如果 Phase 3 在此之前添加了 V003 迁移，则本文件应使用下一个可用编号。实施前请确认实际编号。
>
> `projects.max_concurrent_agents` 列已在 V002 迁移中创建，无需重复添加。

```sql
-- 并行控制历史记录表
CREATE TABLE concurrency_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,  -- 'agent_started', 'agent_completed', 'throttle_on', 'throttle_off'
    agent_id TEXT,
    issue_iid INTEGER,
    issue_title TEXT,
    duration_seconds INTEGER,  -- 仅 agent_completed 时有值
    metadata_json TEXT,        -- 额外元数据（JSON）
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_concurrency_events_project ON concurrency_events(project_id);
CREATE INDEX idx_concurrency_events_type ON concurrency_events(event_type);
CREATE INDEX idx_concurrency_events_created_at ON concurrency_events(created_at);
-- 用于今日统计的复合索引
CREATE INDEX idx_concurrency_events_project_date ON concurrency_events(project_id, created_at);

-- 并行状态快照表（由 watcher 定期写入，用于断电恢复）
CREATE TABLE concurrency_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    active_agents INTEGER NOT NULL DEFAULT 0,
    queued_tasks INTEGER NOT NULL DEFAULT 0,
    agents_json TEXT,  -- JSON array of active agent details
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(project_id)
);

CREATE INDEX idx_concurrency_snapshots_updated ON concurrency_snapshots(updated_at);

-- 更新 system_configs 默认值说明
-- 已有: max_concurrent_codex = 5
-- 新增配置项:
INSERT OR IGNORE INTO system_configs (key, value, description) VALUES
('concurrency_poll_interval_ms', '5000', 'Symphony 实例状态轮询间隔（毫秒）'),
('concurrency_heartbeat_timeout_s', '30', '心跳超时阈值（秒），超时视为实例异常'),
('concurrency_history_retention_days', '30', '并行事件历史保留天数');
```

### 5.2 Schema 关系图

```
┌──────────────┐     ┌─────────────────────┐     ┌──────────────────────┐
│    users     │     │      projects       │     │   project_members    │
│──────────────│     │─────────────────────│     │──────────────────────│
│ id (PK)      │◄────│ created_by (FK)     │     │ id (PK)              │
│ username     │     │ id (PK)             │◄────│ project_id (FK)      │
│ password_hash│     │ name                │     │ user_id (FK)         │──►│users│
│ display_name │     │ max_concurrent_agents│     │ role                 │
│ role         │     │ service_status      │     │ synced_from          │
│ ...          │     │ ...                 │     └──────────────────────┘
└──────────────┘     └─────────────────────┘
       │                      │
       │                      │
       ▼                      ▼
┌──────────────┐     ┌─────────────────────┐
│ user_configs │     │ concurrency_events  │
│──────────────│     │─────────────────────│
│ id (PK)      │     │ id (PK)             │
│ user_id (FK) │     │ project_id (FK)     │
│ gitlab_token │     │ event_type          │
│ gitlab_host  │     │ agent_id            │
│ github_token │     │ issue_iid           │
│ updated_at   │     │ duration_seconds    │
└──────────────┘     │ created_at          │
                     └─────────────────────┘
                              │
                     ┌─────────────────────────┐
                     │ concurrency_snapshots   │
                     │─────────────────────────│
                     │ id (PK)                 │
                     │ project_id (FK, UNIQUE) │
                     │ active_agents           │
                     │ queued_tasks            │
                     │ agents_json             │
                     │ updated_at              │
                     └─────────────────────────┘
```

---

## 6. AppState 扩展

### 6.1 新增 ConcurrencyManager

```rust
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Manages global concurrency state across all Symphony instances.
#[derive(Clone)]
pub struct ConcurrencyManager {
    /// project_id -> current concurrency snapshot (in-memory, updated by watcher)
    snapshots: Arc<DashMap<i64, ProjectSnapshot>>,
    /// Global max from system_configs (cached in memory, updated on config change)
    global_max: Arc<std::sync::atomic::AtomicI64>,
    /// Broadcast channel for SSE events
    event_tx: broadcast::Sender<ConcurrencyEvent>,
    /// Timestamp of last successful watcher poll (for data freshness reporting)
    last_poll_at: Arc<std::sync::Mutex<Option<chrono::DateTime<chrono::Utc>>>>,
    /// One-time SSE tickets: ticket_string -> (user_id, expires_at)
    sse_tickets: Arc<DashMap<String, (i64, chrono::DateTime<chrono::Utc>)>>,
    /// Per-user SSE connection count
    sse_connections_per_user: Arc<DashMap<i64, u32>>,
}

#[derive(Debug, Clone)]
pub struct ProjectSnapshot {
    pub active_agents: i64,
    pub queued_tasks: i64,
    pub agents: Vec<ActiveAgentInfo>,
    pub last_heartbeat: chrono::DateTime<chrono::Utc>,
}

impl ConcurrencyManager {
    pub fn new(global_max: i64) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            snapshots: Arc::new(DashMap::new()),
            global_max: Arc::new(std::sync::atomic::AtomicI64::new(global_max)),
            event_tx,
            last_poll_at: Arc::new(std::sync::Mutex::new(None)),
            sse_tickets: Arc::new(DashMap::new()),
            sse_connections_per_user: Arc::new(DashMap::new()),
        }
    }

    /// Get total active agents across all projects.
    pub fn global_active(&self) -> i64 {
        self.snapshots.iter().map(|e| e.active_agents).sum()
    }

    /// Get total queued tasks across all projects.
    pub fn global_queued(&self) -> i64 {
        self.snapshots.iter().map(|e| e.queued_tasks).sum()
    }

    /// Check if a project is throttled.
    pub fn is_throttled(&self, project_id: i64, project_max: i64) -> (bool, Option<String>) {
        let global_max = self.global_max.load(std::sync::atomic::Ordering::Relaxed);
        let global_active = self.global_active();

        if let Some(snapshot) = self.snapshots.get(&project_id) {
            if snapshot.active_agents >= project_max {
                return (true, Some("project_limit".to_string()));
            }
        }

        if global_active >= global_max {
            return (true, Some("global_limit".to_string()));
        }

        (false, None)
    }

    /// Update snapshot for a project (called by watcher).
    pub fn update_snapshot(&self, project_id: i64, snapshot: ProjectSnapshot) {
        self.snapshots.insert(project_id, snapshot);
    }

    /// Remove snapshot when project stops.
    pub fn remove_snapshot(&self, project_id: i64) {
        self.snapshots.remove(&project_id);
    }

    /// Subscribe to concurrency events (for SSE).
    pub fn subscribe(&self) -> broadcast::Receiver<ConcurrencyEvent> {
        self.event_tx.subscribe()
    }

    /// Publish a concurrency event.
    pub fn publish(&self, event: ConcurrencyEvent) {
        let _ = self.event_tx.send(event);
    }

    /// Update global max (called when admin changes config).
    pub fn set_global_max(&self, max: i64) {
        self.global_max.store(max, std::sync::atomic::Ordering::Relaxed);
    }

    /// Get current global max.
    pub fn get_global_max(&self) -> i64 {
        self.global_max.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Record that a successful poll occurred (for data freshness).
    pub fn mark_poll_success(&self) {
        *self.last_poll_at.lock().unwrap() = Some(chrono::Utc::now());
    }

    /// Get seconds since last successful poll (for data freshness reporting).
    pub fn data_freshness_seconds(&self) -> i64 {
        match *self.last_poll_at.lock().unwrap() {
            Some(t) => (chrono::Utc::now() - t).num_seconds(),
            None => -1, // Never polled yet
        }
    }

    /// Create a one-time SSE ticket for a user (valid 30 seconds).
    pub fn create_sse_ticket(&self, user_id: i64) -> String {
        let ticket = uuid::Uuid::new_v4().to_string();
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(30);
        self.sse_tickets.insert(ticket.clone(), (user_id, expires_at));
        ticket
    }

    /// Validate and consume a one-time SSE ticket. Returns user_id if valid.
    pub fn consume_sse_ticket(&self, ticket: &str) -> Option<i64> {
        if let Some((_, (user_id, expires_at))) = self.sse_tickets.remove(ticket) {
            if chrono::Utc::now() < expires_at {
                return Some(user_id);
            }
        }
        None
    }

    /// Track SSE connection count per user. Returns false if limit exceeded.
    pub fn acquire_sse_slot(&self, user_id: i64, max_per_user: u32) -> bool {
        let mut count = self.sse_connections_per_user.entry(user_id).or_insert(0);
        if *count >= max_per_user {
            return false;
        }
        *count += 1;
        true
    }

    /// Release an SSE connection slot for a user.
    pub fn release_sse_slot(&self, user_id: i64) {
        if let Some(mut count) = self.sse_connections_per_user.get_mut(&user_id) {
            *count = count.saturating_sub(1);
        }
    }
}
```

### 6.2 更新 AppState

```rust
#[derive(Clone)]
pub struct AppState {
    pub repo: SqliteRepository,
    pub jwt_secret: String,
    pub encryption_key: [u8; 32],
    pub token_blacklist: Arc<DashMap<i64, chrono::DateTime<Utc>>>,
    pub rate_limiter: Arc<RateLimiter>,
    pub process_manager: ProcessManager,
    pub api_cache: Arc<ApiCache>,
    pub ai_service: Option<Arc<AiService>>,
    pub phase3_rate_limiter: Arc<Phase3RateLimiter>,
    // Phase 4 additions:
    pub concurrency_manager: ConcurrencyManager,  // 新增
}
```

---

## 7. 新增 Repository Trait 方法

### 7.1 ConcurrencyRepository

```rust
#[async_trait]
pub trait ConcurrencyRepository: Send + Sync {
    /// Record a concurrency event.
    async fn record_concurrency_event(
        &self,
        project_id: i64,
        event_type: &str,
        agent_id: Option<&str>,
        issue_iid: Option<u64>,
        issue_title: Option<&str>,
        duration_seconds: Option<i64>,
        metadata_json: Option<&str>,
    ) -> Result<()>;

    /// Get today's concurrency stats for a project.
    async fn get_today_stats(&self, project_id: i64) -> Result<DailyStats>;

    /// Get today's concurrency stats across all projects.
    async fn get_global_today_stats(&self) -> Result<DailyStats>;

    /// Upsert concurrency snapshot for a project.
    async fn upsert_concurrency_snapshot(
        &self,
        project_id: i64,
        active_agents: i64,
        queued_tasks: i64,
        agents_json: Option<&str>,
    ) -> Result<()>;

    /// Load all concurrency snapshots (for startup recovery).
    async fn load_all_snapshots(&self) -> Result<Vec<(i64, i64, i64, Option<String>, String)>>;

    /// Clean up old concurrency events (retention policy).
    async fn cleanup_old_events(&self, retention_days: i64) -> Result<u64>;
}

#[derive(Debug, Clone)]
pub struct DailyStats {
    pub total_tasks: i64,
    pub avg_duration_seconds: i64,
    pub peak_concurrent: i64,
}
```

### 7.2 TokenStatsRepository

```rust
#[async_trait]
pub trait TokenStatsRepository: Send + Sync {
    /// Count users with GitLab token configured.
    async fn count_users_with_gitlab_token(&self) -> Result<i64>;

    /// Count users with GitHub token configured.
    async fn count_users_with_github_token(&self) -> Result<i64>;

    /// Count active users without any platform token.
    async fn count_users_without_token(&self) -> Result<i64>;

    /// Check if a specific user has a token for the given platform.
    async fn user_has_platform_token(&self, user_id: i64, platform: &str) -> Result<bool>;
}
```

---

## 8. 新增路由注册

```rust
// In router.rs - add to admin_routes:
let admin_routes = Router::new()
    // ... existing admin routes ...
    .route("/api/admin/concurrency", get(concurrency::get_global_concurrency))
    .route("/api/admin/concurrency/config", put(concurrency::update_concurrency_config))
    .route("/api/admin/concurrency/events/ticket", post(concurrency::create_sse_ticket))
    .route("/api/admin/concurrency/events", get(concurrency::concurrency_events_sse))
    .route("/api/admin/stats", get(admin_stats::get_admin_stats))
    .layer(middleware::from_fn(require_admin))
    .layer(middleware::from_fn_with_state(state.clone(), jwt_auth));

// Note: The SSE endpoint itself (/api/admin/concurrency/events) uses ticket-based auth,
// not JWT middleware. It should be registered outside the admin middleware layer,
// or the handler should validate the ticket internally.

// In router.rs - add to project_routes:
let project_routes = Router::new()
    // ... existing project routes ...
    .route("/api/projects/{id}/concurrency", get(concurrency::get_project_concurrency))
    .route("/api/projects/{id}/concurrency", put(concurrency::update_project_concurrency))
    .route("/api/projects/{id}/token-status", get(token_status::get_token_status))
    .route("/api/projects/{id}/contributors", get(contributors::get_contributors))
    .layer(middleware::from_fn_with_state(state.clone(), jwt_auth));

// In router.rs - add to user_routes:
let user_routes = Router::new()
    // ... existing user routes ...
    .route("/api/user/config/validate-token", post(user_profile::validate_token))
    .layer(middleware::from_fn_with_state(state.clone(), jwt_auth));
```

---

## 9. 并行状态聚合机制详细设计

### 9.1 Symphony 实例状态文件协议

每个运行中的 Symphony 实例写入状态文件：

**文件路径**: `/tmp/symphony-{project_id}-status.json`

**文件格式**:
```json
{
  "pid": 12345,
  "active_agents": 2,
  "queued_tasks": 1,
  "agents": [
    {
      "agent_id": "agent-abc123",
      "issue_iid": 42,
      "issue_title": "Implement user auth",
      "started_at": "2025-01-15T10:25:00Z"
    }
  ],
  "updated_at": "2025-01-15T10:30:05Z"
}
```

**写入频率**: Symphony 进程每 3 秒更新一次

### 9.2 Watcher 轮询逻辑

```rust
/// Spawns a background task that polls Symphony instance status files.
pub fn spawn_concurrency_watcher(
    process_manager: ProcessManager,
    concurrency_manager: ConcurrencyManager,
    repo: SqliteRepository,
    poll_interval: Duration,  // default: 5s
    heartbeat_timeout: Duration,  // default: 30s
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(poll_interval);
        loop {
            interval.tick().await;

            // Iterate all running processes
            for entry in process_manager.processes.iter() {
                let project_id = *entry.key();
                let process_state = entry.value().clone();

                if process_state.status != ServiceStatus::Running {
                    continue;
                }

                // Read status file
                let status_path = format!("/tmp/symphony-{}-status.json", project_id);
                match read_status_file(&status_path).await {
                    Ok(status) => {
                        let snapshot = ProjectSnapshot {
                            active_agents: status.active_agents,
                            queued_tasks: status.queued_tasks,
                            agents: status.agents,
                            last_heartbeat: chrono::Utc::now(),
                        };
                        concurrency_manager.update_snapshot(project_id, snapshot);

                        // Persist to DB for crash recovery
                        let _ = repo.upsert_concurrency_snapshot(
                            project_id,
                            status.active_agents,
                            status.queued_tasks,
                            serde_json::to_string(&status.agents).ok().as_deref(),
                        ).await;
                    }
                    Err(_) => {
                        // Check if heartbeat timed out
                        if let Some(existing) = concurrency_manager.snapshots.get(&project_id) {
                            let elapsed = chrono::Utc::now()
                                .signed_duration_since(existing.last_heartbeat);
                            if elapsed > chrono::Duration::from_std(heartbeat_timeout).unwrap() {
                                // Mark as stale - zero out agents
                                concurrency_manager.update_snapshot(project_id, ProjectSnapshot {
                                    active_agents: 0,
                                    queued_tasks: 0,
                                    agents: vec![],
                                    last_heartbeat: existing.last_heartbeat,
                                });
                            }
                        }
                    }
                }
            }

            // Clean up snapshots for stopped projects
            let active_projects: Vec<i64> = process_manager.processes
                .iter()
                .map(|e| *e.key())
                .collect();

            concurrency_manager.snapshots.retain(|k, _| active_projects.contains(k));
        }
    });
}
```

### 9.3 Race Condition 处理

| 场景 | 问题 | 解决方案 |
|------|------|----------|
| 并发读写 global_max | 多个请求同时修改 | AtomicI64 保证原子性 |
| 状态文件读写竞争 | Watcher 读取时 Symphony 正在写入 | 文件写入使用 rename 原子操作 |
| 快照过期判定 | 时钟偏移导致误判 | 使用单调时钟 + 30s 宽容窗口 |
| SSE 事件丢失 | broadcast channel 满 | channel 容量 256，lagging receiver 自动跳过 |
| 启动时恢复 | Web Platform 重启后状态丢失 | 从 concurrency_snapshots 表恢复 + 立即轮询 |
| 多实例同项目 | 同一项目多个 Symphony 进程 | 当前设计一个项目只有一个进程（ProcessManager 保证） |

---

## 10. GitPlatformClient Trait 扩展

### 10.1 新增方法

```rust
#[async_trait]
pub trait GitPlatformClient: Send + Sync {
    // ... 现有方法 ...

    /// Validate a token by calling the platform's user info endpoint.
    /// Returns the authenticated user's info if valid.
    async fn validate_token(
        &self,
        token: &str,
    ) -> Result<TokenValidationResult, GitPlatformError>;

    /// Get project contributors (aggregated from issues and MRs).
    async fn get_contributors(
        &self,
        token: &str,
        project_path: &str,
        limit: u32,
    ) -> Result<Vec<PlatformContributor>, GitPlatformError>;
}
```

> **注意**: 不新增 `list_issues_with_author` 方法。作者筛选通过扩展现有 `ListIssuesOptions` 的 `author` 字段实现（见 10.2），复用现有 `list_issues` 方法即可。

#[derive(Debug, Clone)]
pub struct TokenValidationResult {
    pub valid: bool,
    pub username: Option<String>,
    pub scopes: Vec<String>,
    pub expires_at: Option<String>,
    pub error_detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PlatformContributor {
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub is_bot: bool,
}
```

### 10.2 ListIssuesOptions 扩展

```rust
#[derive(Debug, Clone, Default)]
pub struct ListIssuesOptions {
    pub labels: Option<Vec<String>>,
    pub exclude_labels: Option<Vec<String>>,
    pub assignee: Option<String>,
    pub author: Option<String>,  // 新增: 按作者筛选
    pub search: Option<String>,
    pub limit: u32,
    pub state: Option<String>,
}
```

---

## 11. 完整端点汇总

### Phase 4 新增端点

| 方法 | 路径 | 权限 | 说明 |
|------|------|------|------|
| POST | /api/user/config/validate-token | 登录用户 | 验证平台 Token 有效性 |
| GET | /api/projects/:id/token-status | 项目成员 | 查看项目 Token 配置状态 |
| GET | /api/admin/concurrency | Admin | 全局并行状态 |
| PUT | /api/admin/concurrency/config | Admin | 更新全局并行配置 |
| POST | /api/admin/concurrency/events/ticket | Admin | 获取 SSE 连接一次性 ticket |
| GET | /api/admin/concurrency/events | Admin (ticket) | SSE 实时并行事件 |
| GET | /api/projects/:id/concurrency | 项目成员 | 项目并行状态 |
| PUT | /api/projects/:id/concurrency | Owner/Admin | 更新项目并行配置 |
| GET | /api/projects/:id/contributors | 项目成员 | 项目贡献者列表 |
| GET | /api/admin/stats | Admin | 管理统计（增强） |

### Phase 4 增强的现有端点

| 方法 | 路径 | 变更 |
|------|------|------|
| GET | /api/projects/:id/kanban | 新增 `author` 查询参数 |
| GET | /api/projects/:id/mrs/:iid | 新增 `logical_author` 响应字段 |
| POST | /api/projects/:id/start | 新增 Owner Token 校验 |

---

## 12. 安全考虑

### 12.1 Token 安全

- 平台 Token 始终加密存储（AES-256-GCM，使用 `encryption_key`）
- Token 验证端点不缓存 Token 明文，验证后立即丢弃
- Token 验证端点添加最低 500ms 响应时间，防止时序攻击推断 Token 有效性
- Token 验证端点限速 3 次/分钟/用户，防止 Token 枚举
- SSE 端点使用一次性 ticket 认证（30 秒有效期），避免 JWT 通过 URL 泄露
- Token 状态查询不暴露 Token 内容，仅返回 `has_token: bool`

### 12.2 并行控制安全

- `global_max` 修改仅 Admin 可操作
- 项目级 `max_agents` 修改需要 Owner 权限
- 状态文件路径使用 project_id 构造，不接受用户输入（防止路径遍历）
- SSE 连接数限制防止资源耗尽

### 12.3 数据隔离

- 普通成员查看 token-status 时只能看到自己的状态
- 贡献者列表使用当前用户的 Token 获取（只能看到自己权限范围内的数据）
- 并行事件历史按项目隔离，成员只能查看所属项目的数据

---

## 13. 性能考虑

### 13.1 缓存策略

| 数据 | 缓存位置 | TTL | 说明 |
|------|----------|-----|------|
| 并行状态 | ConcurrencyManager (内存) | 实时 (5s 轮询) | DashMap 无锁读 |
| 贡献者列表 | ApiCache | 60s | singleflight 防击穿 |
| Token 验证结果 | 不缓存 | - | 安全考虑，每次实时验证 |
| 今日统计 | 内存 + DB | 10s | 避免频繁 COUNT 查询 |

### 13.2 数据库查询优化

- `concurrency_events` 表使用复合索引 `(project_id, created_at)` 加速今日统计
- `concurrency_snapshots` 表使用 `UNIQUE(project_id)` 实现 upsert
- 历史数据定期清理（默认保留 30 天）

### 13.3 SSE 连接管理

- 使用 `broadcast::channel` 实现一对多事件分发
- Channel 容量 256，慢消费者自动 lag（不阻塞生产者）
- 最大 10 个并发 SSE 连接（使用 Semaphore 控制）
- 30 秒 heartbeat 防止连接被中间代理断开
