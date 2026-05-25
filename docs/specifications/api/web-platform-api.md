# Web Platform REST API 参考文档

## 概述

Web Platform 是 Symphony 的管理后端，基于 Axum 0.8 构建，提供项目管理、服务控制、用户管理等 REST API。

- **Base URL**：`/api`
- **认证方式**：Bearer JWT（除登录接口外，所有接口均需携带 `Authorization: Bearer <token>` 请求头）
- **Swagger UI**：`/swagger-ui`（OpenAPI JSON：`/api-docs/openapi.json`）
- **健康检查**：`GET /health`（无需认证）

### 通用响应格式

所有接口均返回统一的 JSON 信封：

```json
{
  "data": <响应数据>,
  "success": true,
  "retCode": "0",
  "retMsg": "ok",
  "showType": null
}
```

失败时：

```json
{
  "data": null,
  "success": false,
  "retCode": "AUTH_001",
  "retMsg": "Authentication required",
  "showType": 9
}
```

`showType` 含义：`1` = 静默、`2` = 警告弹窗、`4` = 通知、`9` = 跳转登录页。

### 分页响应格式

列表接口返回 `PaginationData`：

```json
{
  "data": {
    "records": [...],
    "totalCount": 100,
    "pageNo": 1,
    "pageSize": 20,
    "pages": 5,
    "limit": 20,
    "offset": 0
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

---

## 认证接口

### POST /api/auth/login

用户登录，返回 JWT token。无需认证。

**请求体：**

```json
{
  "username": "admin",
  "password": "your_password"
}
```

**响应 data：**

```json
{
  "token": "eyJ...",
  "user": {
    "id": 1,
    "username": "admin",
    "displayName": "管理员",
    "role": "admin"
  }
}
```

**错误码：**
- `AUTH_003`（401）：用户名或密码错误

---

### PUT /api/auth/password

修改当前登录用户的密码。需要 JWT 认证。

**请求体：**

```json
{
  "old_password": "current_password",
  "new_password": "new_password"
}
```

**响应 data：** `null`（成功时）

---

## 用户接口

### GET /api/user/profile

获取当前用户的个人信息。

**响应 data：**

```json
{
  "id": 1,
  "username": "admin",
  "displayName": "管理员",
  "role": "admin",
  "createdAt": "2024-01-01T00:00:00Z"
}
```

---

### PUT /api/user/profile

更新当前用户的个人信息（如 displayName）。

**请求体：**

```json
{
  "display_name": "新名称"
}
```

---

### GET /api/user/config

获取当前用户的平台配置（GitLab/GitHub token 等）。敏感字段（token）在响应中脱敏显示。

**响应 data：**

```json
{
  "gitlabToken": "gl***",
  "gitlabHost": "https://gitlab.example.com",
  "githubToken": "gh***"
}
```

---

### PUT /api/user/config

更新当前用户的平台配置。token 字段在数据库中使用 AES-256-GCM 加密存储。

**请求体：**

```json
{
  "gitlab_token": "glpat-xxxx",
  "gitlab_host": "https://gitlab.example.com",
  "github_token": "ghp_xxxx"
}
```

---

### POST /api/user/config/validate-token

验证用户配置的平台 token 是否有效（实际调用对应平台 API 验证）。

**请求体：**

```json
{
  "platform": "gitlab",
  "token": "glpat-xxxx",
  "host": "https://gitlab.example.com"
}
```

**响应 data：**

```json
{
  "valid": true,
  "username": "john.doe",
  "message": "Token valid"
}
```

**错误码：**
- `TOKEN_001`（400）：Token 无效或已过期

---

## 管理员接口

以下接口均需要 `role = admin` 权限，否则返回 `AUTH_002`（403）。

### 用户管理

#### GET /api/admin/users

获取用户列表（支持分页）。

**查询参数：** `page`、`page_size`

**响应 data：** `PaginationData<UserInfo>`

`UserInfo` 字段：`id`、`username`、`displayName`、`role`、`createdAt`、`deletedAt`

---

#### POST /api/admin/users

创建新用户。

**请求体：**

```json
{
  "username": "newuser",
  "password": "initial_password",
  "role": "user",
  "display_name": "新用户"
}
```

---

#### DELETE /api/admin/users/{id}

软删除用户（设置 `deleted_at`，不物理删除）。

---

#### PUT /api/admin/users/{id}/reset-password

重置指定用户密码。

**请求体：**

```json
{
  "new_password": "reset_password"
}
```

---

### 并发控制

#### GET /api/admin/concurrency

获取全局并发状态，包括当前运行中的 agent 数量、队列状态、历史事件统计。

#### PUT /api/admin/concurrency/config

更新全局并发配置（`max_concurrent_codex` 等 system_configs 键值）。

#### POST /api/admin/concurrency/events/ticket

创建 SSE 订阅票据（ticket），用于后续 SSE 连接认证。

**响应 data：**

```json
{
  "ticket": "uuid-ticket-string",
  "expires_at": "2024-01-01T00:01:00Z"
}
```

#### GET /api/admin/concurrency/events

SSE 流式端点，使用 ticket 参数认证（不使用 JWT middleware）。

**查询参数：** `ticket=<ticket_value>`

推送并发事件（agent 启动/完成/限流等）。

---

### 告警规则

#### GET /api/admin/alerts

获取告警历史列表（支持分页、按 severity/project_id 筛选）。

#### GET /api/admin/alerts/rules

获取所有告警规则配置。预置规则：`task_timeout`、`task_failure`、`service_crash`、`concurrency_saturation`、`consecutive_failures`、`api_unreachable`。

#### PUT /api/admin/alerts/rules

批量更新告警规则（enabled、threshold_json、cooldown_seconds 等）。

---

### 告警通道

#### GET /api/admin/alerts/channels

获取所有通知渠道配置。`config_encrypted` 字段在响应中脱敏。

#### PUT /api/admin/alerts/channels

更新通知渠道配置（支持 DingTalk 等渠道类型）。

#### POST /api/admin/alerts/test

发送测试通知，验证渠道配置是否正确。

**请求体：**

```json
{
  "channel_id": "dingtalk-main"
}
```

**错误码：**
- `ALERT_001`（404）：告警规则不存在
- `ALERT_002`（400）：通知渠道配置无效
- `ALERT_003`（502）：通知发送失败

---

### 系统配置

#### GET /api/admin/config

获取 `system_configs` 表中的所有键值配置。

#### PUT /api/admin/config

更新系统配置键值。

#### GET /api/admin/stats

获取系统统计信息（项目数、用户数、运行中服务数等）。

---

### 网络代理

#### GET /api/admin/network-proxy

获取当前网络代理配置（mode、no_proxy、auto_bypass_local 等）。

#### PUT /api/admin/network-proxy

更新网络代理配置。代理 URL 等敏感字段存储在 `secret_configs` 表（AES-256-GCM 加密）。

**请求体：**

```json
{
  "mode": "manual",
  "http_proxy": "http://proxy.example.com:8080",
  "https_proxy": "http://proxy.example.com:8080",
  "no_proxy": "localhost,127.0.0.1",
  "auto_bypass_local": true
}
```

代理模式枚举：`disabled`、`inherit_env`、`manual`

#### GET /api/admin/network-proxy/effective

获取当前实际生效的代理配置（合并环境变量与数据库配置后的结果）。

#### POST /api/admin/network-proxy/test

测试代理连通性。

---

## 项目接口

### 项目 CRUD

#### GET /api/projects

获取当前用户有权限的项目列表。

**响应 data：** `PaginationData<Project>`

Project 核心字段：`id`、`name`、`description`、`gitUrl`、`platform`、`platformHost`、`namespace`、`repoName`、`defaultBranch`、`serviceStatus`、`createdAt`、`updatedAt`

`serviceStatus` 枚举：`stopped`、`starting`、`running`、`stopping`、`error`

---

#### POST /api/projects

创建新项目。

**请求体：**

```json
{
  "name": "My Project",
  "description": "项目描述",
  "git_url": "https://gitlab.example.com/group/repo.git",
  "platform": "gitlab",
  "platform_host": "https://gitlab.example.com",
  "default_branch": "main",
  "workflow_template": "default"
}
```

`platform` 枚举：`gitlab`、`github`

---

#### GET /api/projects/{id}

获取单个项目详情。

#### PUT /api/projects/{id}

更新项目信息（name、description、workflow_content 等）。

#### DELETE /api/projects/{id}

删除项目（需先停止服务）。

---

### 服务控制

#### POST /api/projects/{id}/start

启动项目的 rust-platform 服务进程。

**响应 data：**

```json
{
  "status": "starting",
  "pid": null,
  "message": "Service start initiated"
}
```

#### POST /api/projects/{id}/stop

停止服务进程（发送 SIGTERM，等待优雅退出）。

#### POST /api/projects/{id}/restart

重启服务进程（stop + start）。

#### GET /api/projects/{id}/status

获取服务当前状态。

**响应 data：**

```json
{
  "status": "running",
  "pid": 12345,
  "startedAt": "2024-01-01T10:00:00Z",
  "generation": 3,
  "instanceId": "uuid-instance"
}
```

#### GET /api/projects/{id}/diagnostics

获取服务诊断信息（进程状态、配置校验结果、最近日志摘要等）。

---

### 成员管理

#### GET /api/projects/{id}/members

获取项目成员列表。

**响应 data：** 成员数组，每项包含 `userId`、`username`、`role`、`syncedFrom`

#### POST /api/projects/{id}/members

添加项目成员。

**请求体：**

```json
{
  "user_id": 2,
  "role": "member"
}
```

`role` 枚举：`owner`、`member`

#### PUT /api/projects/{id}/members/{userId}

更新成员角色。

#### DELETE /api/projects/{id}/members/{userId}

移除项目成员。

#### POST /api/projects/{id}/members/sync

从 GitLab/GitHub 同步项目成员（需要用户配置了对应平台 token）。

---

### Workflow 配置

#### GET /api/projects/{id}/workflow

获取项目的 WORKFLOW.md 内容。

**响应 data：**

```json
{
  "content": "---\ntracker:\n  kind: gitlab\n...",
  "template": "default",
  "updatedAt": "2024-01-01T00:00:00Z"
}
```

#### PUT /api/projects/{id}/workflow

更新 WORKFLOW.md 内容。

**请求体：**

```json
{
  "content": "---\ntracker:\n  kind: gitlab\n  api_key: $GITLAB_TOKEN\n..."
}
```

#### POST /api/projects/{id}/workflow/reset

将 WORKFLOW.md 重置为默认模板。

---

### 看板

#### GET /api/projects/{id}/kanban

获取项目看板数据（从 GitLab/GitHub 实时拉取）。

**响应 data：**

```json
{
  "columns": [
    {
      "state": "Todo",
      "issues": [...]
    },
    {
      "state": "In Progress",
      "issues": [...]
    }
  ],
  "updatedAt": "2024-01-01T10:00:00Z"
}
```

---

### Issue 管理

#### POST /api/projects/{id}/issues

在 GitLab/GitHub 创建新 Issue。

**请求体：**

```json
{
  "title": "Issue 标题",
  "description": "Issue 描述",
  "labels": ["Backlog"]
}
```

#### GET /api/projects/{id}/issues/{iid}

获取单个 Issue 详情（iid 为平台内部编号）。

#### GET /api/projects/{id}/issues/{iid}/mrs

获取与指定 Issue 关联的 MR 列表。

---

### MR 管理

#### POST /api/projects/{id}/mrs

创建 Merge Request（支持幂等性，通过 `Idempotency-Key` 请求头去重）。

**请求头：** `Idempotency-Key: <unique-key>`（可选，用于幂等创建）

**请求体：**

```json
{
  "source_branch": "feature/abc-123",
  "target_branch": "main",
  "title": "Fix: resolve issue ABC-123",
  "description": "MR 描述"
}
```

#### GET /api/projects/{id}/mrs/{iid}

获取单个 MR 详情。

---

### 贡献者

#### GET /api/projects/{id}/contributors

获取项目贡献者统计（从平台 API 拉取）。

---

### 项目并发

#### GET /api/projects/{id}/concurrency

获取项目级别的并发状态（当前运行 agent 数、队列任务数、今日统计）。

#### PUT /api/projects/{id}/concurrency

更新项目级别的并发配置（`max_concurrent_agents`）。

---

## AI 生成接口

### POST /api/projects/{id}/issues/ai-generate

使用 AI 生成 Issue 内容（SSE 流式响应）。

**请求体：**

```json
{
  "prompt": "实现用户登录功能，支持 JWT 认证"
}
```

**响应：** `Content-Type: text/event-stream`

SSE 事件格式：

```
data: {"type":"chunk","content":"## 功能描述\n"}

data: {"type":"chunk","content":"实现基于 JWT 的用户登录..."}

data: {"type":"done","title":"实现用户登录功能","description":"...完整内容..."}
```

**限流：** 10 次/分钟，超限返回 `EXT_002`（429），响应头包含 `Retry-After`。

---

## 错误码体系

| 错误码 | HTTP 状态 | 含义 | showType |
|--------|-----------|------|----------|
| `AUTH_001` | 401 | 未认证（需要登录） | 9（跳转登录） |
| `AUTH_002` | 403 | 权限不足（非管理员） | 2 |
| `AUTH_003` | 401 | 用户名或密码错误 | 2 |
| `BIZ_001` | 400 | 请求参数错误 | 1 |
| `BIZ_002` | 404 | 资源不存在 | 2 |
| `BIZ_003` | 409 | 资源冲突（如用户名重复） | 1 |
| `TOKEN_001` | 400 | 平台 token 无效或过期 | 1 |
| `EXT_001` | 502 | 外部服务不可用（GitLab/GitHub/AI） | 4 |
| `EXT_002` | 429 | 请求频率超限 | 1 |
| `ALERT_001` | 404 | 告警规则不存在 | 2 |
| `ALERT_002` | 400 | 通知渠道配置无效 | 1 |
| `ALERT_003` | 502 | 通知发送失败 | 1 |
| `SYS_001` | 500 | 内部服务器错误 | 2 |

限流响应（`EXT_002`）会附带 `Retry-After` 响应头，值为建议等待秒数。

---

## Swagger UI

访问 `/swagger-ui` 可查看交互式 API 文档。OpenAPI JSON 规范位于 `/api-docs/openapi.json`。

当前 Swagger 文档覆盖认证、用户、管理员用户管理接口。其余接口可通过本文档参考。
