# rust-platform 内部 HTTP API 参考文档

## 概述

rust-platform 是 Symphony 的核心编排引擎，负责轮询 Issue Tracker、调度 Codex agent 子进程、管理工作区生命周期。它暴露一个轻量级 HTTP API，供 web-platform 进程管理器调用，也可供运维人员直接访问进行状态查询和手动触发。

**调用方**：web-platform 的进程管理器（`process_manager/spawn.rs`）在启动 rust-platform 子进程后，通过此 API 查询状态。

**端口配置**：通过 WORKFLOW.md 的 `server.port` 字段配置，例如：

```yaml
server:
  port: 8765
```

若未配置 `server.port`，HTTP 服务不启动。

**认证**：无认证（内部 API，仅监听本地端口，不对外暴露）。

**超时**：HTTP handler 向 orchestrator 发送查询后，等待最多 5 秒，超时返回 `503 Service Unavailable`。

---

## 接口列表

### GET /

返回人类可读的 Dashboard HTML 页面，展示当前系统状态。

**响应**：`Content-Type: text/html`

页面内容包括：
- 统计卡片：运行中 agent 数、重试队列数、总 token 消耗、累计运行时长
- 运行中会话表格：Issue 标识、状态、Session ID、Turn 数、最后事件、启动时间
- 重试队列表格：Issue 标识、重试次数、预计执行时间、错误信息

若 orchestrator 不可用，返回 `503` 并显示简单错误页面。

---

### GET /api/v1/state

获取系统完整状态快照。

**响应**：`200 OK`，`Content-Type: application/json`

**响应体（StateResponse）：**

```json
{
  "generated_at": "2024-01-15T10:00:00Z",
  "counts": {
    "running": 2,
    "retrying": 1
  },
  "running": [
    {
      "issue_id": "uuid-string",
      "issue_identifier": "PROJ-42",
      "state": "In Progress",
      "session_id": "thread-abc-turn-xyz",
      "turn_count": 5,
      "last_event": "notification",
      "last_message": "Writing unit tests...",
      "started_at": "2024-01-15T09:50:00Z",
      "last_event_at": "2024-01-15T09:59:30Z",
      "tokens": {
        "input_tokens": 12000,
        "output_tokens": 8000,
        "total_tokens": 20000
      }
    }
  ],
  "retrying": [
    {
      "issue_id": "uuid-string-2",
      "issue_identifier": "PROJ-43",
      "attempt": 2,
      "due_at": "2024-01-15T10:05:00Z",
      "error": "turn timeout exceeded"
    }
  ],
  "codex_totals": {
    "input_tokens": 50000,
    "output_tokens": 24000,
    "total_tokens": 74000,
    "seconds_running": 1834.2
  },
  "rate_limits": null
}
```

**错误响应（503）：**

```json
{
  "error": {
    "code": "unavailable",
    "message": "orchestrator unavailable or timed out"
  }
}
```

---

### GET /api/v1/{identifier}

获取指定 Issue 的详细状态。`identifier` 为 Issue 的人类可读标识（如 `PROJ-42`、`ABC-123`）。

**响应**：`200 OK`

**响应体（IssueDetailResponse）：**

```json
{
  "issue_identifier": "PROJ-42",
  "issue_id": "uuid-string",
  "status": "running",
  "running": {
    "issue_id": "uuid-string",
    "issue_identifier": "PROJ-42",
    "state": "In Progress",
    "session_id": "thread-abc-turn-xyz",
    "turn_count": 5,
    "last_event": "notification",
    "last_message": "Writing unit tests...",
    "started_at": "2024-01-15T09:50:00Z",
    "last_event_at": "2024-01-15T09:59:30Z",
    "tokens": {
      "input_tokens": 12000,
      "output_tokens": 8000,
      "total_tokens": 20000
    }
  },
  "retry": null,
  "last_error": null
}
```

`status` 可能值：`running`、`retrying`、`idle`（不在运行或重试队列中）

**错误响应（404）：**

```json
{
  "error": {
    "code": "issue_not_found",
    "message": "issue 'PROJ-42' not found in current state"
  }
}
```

---

### POST /api/v1/refresh

触发立即轮询 + 协调周期（fire-and-forget，不等待执行完成）。

**请求体**：无

**响应**：`202 Accepted`

```json
{
  "queued": true,
  "coalesced": false,
  "requested_at": "2024-01-15T10:00:00Z",
  "operations": ["poll", "reconcile"]
}
```

此接口向 orchestrator 发送 `ForceRefresh` 事件后立即返回，不等待轮询完成。

---

## 响应类型定义

### StateResponse

| 字段 | 类型 | 说明 |
|------|------|------|
| `generated_at` | string (RFC3339) | 快照生成时间 |
| `counts` | Counts | 运行/重试数量汇总 |
| `running` | RunningRow[] | 当前运行中的会话列表 |
| `retrying` | RetryRow[] | 重试队列列表 |
| `codex_totals` | CodexTotalsJson | 累计 token 和运行时统计 |
| `rate_limits` | object \| null | Codex API 速率限制信息（原始 JSON） |

### RunningRow

| 字段 | 类型 | 说明 |
|------|------|------|
| `issue_id` | string | Issue 的内部 UUID |
| `issue_identifier` | string | Issue 的人类可读标识（如 PROJ-42） |
| `state` | string | Issue 在 tracker 中的当前状态 |
| `session_id` | string | 组合会话 ID（`{thread_id}-{turn_id}`） |
| `turn_count` | number | 当前会话已完成的 turn 数 |
| `last_event` | string \| null | 最后收到的 Codex 事件类型 |
| `last_message` | string \| null | 最后收到的 Codex 消息摘要（截断至 200 字符） |
| `started_at` | string (RFC3339) | 会话启动时间（UTC） |
| `last_event_at` | string \| null | 最后事件时间（UTC） |
| `tokens` | TokensJson | 本会话 token 消耗 |

### RetryRow

| 字段 | 类型 | 说明 |
|------|------|------|
| `issue_id` | string | Issue 的内部 UUID |
| `issue_identifier` | string | Issue 的人类可读标识 |
| `attempt` | number | 当前重试次数（1-based） |
| `due_at` | string (RFC3339) | 预计执行时间 |
| `error` | string \| null | 导致重试的错误信息 |

### IssueDetailResponse

| 字段 | 类型 | 说明 |
|------|------|------|
| `issue_identifier` | string | Issue 标识 |
| `issue_id` | string | Issue UUID |
| `status` | string | `running` / `retrying` / `idle` |
| `running` | RunningRow \| null | 运行中会话详情（status=running 时有值） |
| `retry` | RetryRow \| null | 重试队列详情（status=retrying 时有值） |
| `last_error` | string \| null | 最后一次错误信息 |

### CodexTotalsJson

| 字段 | 类型 | 说明 |
|------|------|------|
| `input_tokens` | number | 累计输入 token 数 |
| `output_tokens` | number | 累计输出 token 数 |
| `total_tokens` | number | 累计总 token 数 |
| `seconds_running` | number | 累计运行时长（秒，浮点数） |

### TokensJson

| 字段 | 类型 | 说明 |
|------|------|------|
| `input_tokens` | number | 输入 token 数 |
| `output_tokens` | number | 输出 token 数 |
| `total_tokens` | number | 总 token 数 |
