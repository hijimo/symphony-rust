# Codex App-Server JSON-line stdio 协议文档

## 概述

Codex 作为子进程由 rust-platform 的 `CodexClient` 启动，通过 stdin/stdout 进行 JSON-line 通信，遵循 JSON-RPC 2.0 协议。每条消息为一行 JSON，以换行符 `\n` 结尾。

**启动方式**：

```bash
bash -lc "<codex.command>"
```

其中 `codex.command` 来自 WORKFLOW.md 的 `codex.command` 字段，默认值为 `codex app-server`。

子进程以 `process_group(0)` 启动，便于通过负 PID 向整个进程组发送信号。

**stderr 处理**：stderr 设为 `Stdio::piped()`，由后台任务持续 drain，防止 pipe buffer（64KB）满导致子进程阻塞。详见编码规范中的"子进程 stderr 必须被消费"。

**环境变量继承**：子进程继承代理相关环境变量（`http_proxy`、`https_proxy`、`all_proxy` 及大写形式、`no_proxy`、`NO_PROXY`、`SYMPHONY_PROXY_*`），确保 Codex 内部的 shell 工具能通过代理访问外部 API。

---

## 生命周期

```
启动子进程
    │
    ▼
initialize (握手第一步)
    │
    ▼
thread/start (握手第二步，获取 thread_id)
    │
    ▼
turn/start (发送任务 prompt)
    │
    ▼
← 事件流 (JSON-line 通知)
    │
    ├── turn.completed → 本次 turn 成功
    ├── turn.failed    → 本次 turn 失败
    ├── turn.cancelled → turn 被取消
    ├── turn.input_required → 需要用户输入（高信任模式下视为失败）
    └── turn.error     → turn 出错
    │
    ▼
下一个 turn/start（复用同一 thread_id）
    │
    ▼
stop（优雅关闭，5 秒超时后 SIGKILL）
```

**握手（Handshake）**：每个 `CodexClient` 实例在第一次 `turn/start` 前执行一次握手：先发送 `initialize`，等待响应；再发送 `thread/start`，从响应中提取 `thread_id`。后续 turn 复用同一 `thread_id`，直接发送 `turn/start`。

---

## 请求消息类型

所有请求均为 JSON-RPC 2.0 格式，写入子进程 stdin，每条消息一行。

### initialize

握手第一步，声明客户端信息。

```json
{
  "jsonrpc": "2.0",
  "method": "initialize",
  "params": {
    "clientInfo": {
      "name": "symphony-platform",
      "version": "0.1.0"
    }
  },
  "id": 1
}
```

**响应**：等待 Codex 返回对应 `id` 的 JSON-RPC 响应，超时由 `read_timeout_ms` 控制（默认 5000ms）。

---

### thread/start

握手第二步，创建新线程并配置沙箱策略。

```json
{
  "jsonrpc": "2.0",
  "method": "thread/start",
  "params": {
    "cwd": "/path/to/workspace",
    "approvalPolicy": "never",
    "sandbox": "workspace-write"
  },
  "id": 2
}
```

**参数说明：**

| 参数 | 类型 | 说明 |
|------|------|------|
| `cwd` | string | 工作区路径 |
| `approvalPolicy` | string \| object | 审批策略，来自 `codex.approval_policy` 配置 |
| `sandbox` | string \| object | 沙箱策略，来自 `codex.turn_sandbox_policy` 配置（直接传递原始值） |

**响应**：从响应中提取 `thread_id`，支持多种路径：`result.thread.id`、`result.threadId`、`params.threadId` 等。

---

### turn/start

发起一次 AI 编码任务。

```json
{
  "jsonrpc": "2.0",
  "method": "turn/start",
  "params": {
    "cwd": "/path/to/workspace",
    "input": [
      {
        "type": "text",
        "text": "Fix issue ABC-123: implement user authentication"
      }
    ],
    "threadId": "thread-abc-123",
    "sandboxPolicy": {
      "type": "workspaceWrite"
    }
  },
  "id": 3
}
```

**参数说明：**

| 参数 | 类型 | 说明 |
|------|------|------|
| `cwd` | string | 工作区路径 |
| `input` | array | 输入内容数组，每项包含 `type` 和 `text` |
| `threadId` | string | 线程 ID（握手后获取，复用） |
| `sandboxPolicy` | object | Turn 级别沙箱策略（camelCase 格式） |

注意：`sandboxPolicy` 的值由 `codex.turn_sandbox_policy` 配置转换而来，kebab-case 字符串会转换为 camelCase 对象，例如 `"workspace-write"` → `{"type": "workspaceWrite"}`。

---

### cancel

取消当前 turn（通过 `CancellationToken` 触发，发送后立即 kill 进程组）。

```json
{"type": "stop"}
```

优雅停止时先发送此消息，等待 5 秒，超时后发送 SIGKILL 到整个进程组。

---

### approval/resolve

自动审批 Codex 发出的审批请求（`approval_policy: never` 模式下自动批准）。

```json
{
  "jsonrpc": "2.0",
  "method": "approval/resolve",
  "params": {
    "id": "request-id",
    "approved": true
  },
  "id": 4
}
```

---

## 事件消息类型

Codex 通过 stdout 输出 JSON-line 事件流，每行一个 JSON 对象。

### CodexEventUpdate（内部结构）

`CodexClient` 从原始事件中提取并归一化为 `CodexEventUpdate`：

| 字段 | 类型 | 说明 |
|------|------|------|
| `event_type` | string \| null | 事件类型字符串 |
| `message` | string \| null | 消息内容摘要（截断至 200 字符） |
| `input_tokens` | number \| null | 本事件的输入 token 数 |
| `output_tokens` | number \| null | 本事件的输出 token 数 |
| `total_tokens` | number \| null | 本事件的总 token 数 |
| `timestamp` | string \| null | 事件时间戳（RFC3339） |
| `rate_limits` | object \| null | 速率限制信息（原始 JSON） |
| `pid` | string \| null | Codex 子进程 PID |
| `session_id` | string \| null | 组合会话 ID（`{thread_id}-{turn_id}`） |
| `thread_id` | string \| null | 线程 ID |
| `turn_id` | string \| null | Turn ID |

### 终止事件类型

以下事件类型会终止当前 turn 的事件流循环：

| 事件类型 | 含义 | 处理方式 |
|----------|------|----------|
| `turn.completed` / `turn_completed` / `turn/completed` | Turn 成功完成 | 返回 `TurnResult::Completed` |
| `turn.failed` / `turn_failed` / `turn/failed` | Turn 失败 | 返回 `TurnResult::Failed { reason }` |
| `turn.cancelled` / `turn_cancelled` / `turn/cancelled` | Turn 被取消 | 返回 `CodexError::TurnCancelled` |
| `turn.input_required` / `input_required` | 需要用户输入 | 返回 `CodexError::TurnInputRequired`（高信任模式视为失败，触发重试） |
| `turn.error` / `turn_ended_with_error` / `turn/error` | Turn 出错 | 返回 `CodexError::TurnFailed { reason }` |

### 审批请求事件

以下事件类型触发自动审批响应（不终止 turn）：

- `approval/request`
- `approval_request`
- `commandExecution`
- `fileChange`

### 工具调用事件

以下事件类型触发工具调用响应（当前返回 `tool not available` 错误）：

- `item/tool/call`
- `tool/call`
- `tool_call`

---

## 会话管理

### 层级关系

```
CodexClient 实例
└── thread_id（握手时由 thread/start 响应获取，整个实例生命周期内固定）
    └── turn_id（每次 turn/start 后从事件流中提取，每个 turn 不同）
        └── session_id = "{thread_id}-{turn_id}"
```

- **thread_id**：从 `thread/start` 响应中提取，支持多种字段路径（`result.thread.id`、`result.threadId`、`params.threadId` 等）
- **turn_id**：从事件流中提取，支持多种字段路径（`params.turn.id`、`params.turnId` 等）
- **session_id**：由 `compose_session_id(thread_id, turn_id)` 生成，格式为 `{thread_id}-{turn_id}`

---

## 沙箱策略

沙箱策略通过 WORKFLOW.md 配置：

```yaml
codex:
  approval_policy: "never"
  turn_sandbox_policy: "workspace-write"
```

**approval_policy** 传入 `thread/start` 的 `approvalPolicy` 字段，值直接传递（字符串或对象）。

**turn_sandbox_policy** 同时用于两个阶段，但传递方式不同：
- `thread/start` 的 `sandbox` 字段：直接传递原始值（如字符串 `"workspace-write"`）
- `turn/start` 的 `sandboxPolicy` 字段：转换为 camelCase 对象格式（如 `{"type": "workspaceWrite"}`）

> 注：配置中存在 `thread_sandbox` 字段但当前代码未使用，实际 `thread/start` 的 sandbox 值来自 `turn_sandbox_policy`。

**turn_sandbox_policy** 传入 `turn/start` 的 `sandboxPolicy` 字段，字符串值会转换为 camelCase 对象：

| 配置值 | 转换结果 |
|--------|----------|
| `"workspace-write"` | `{"type": "workspaceWrite"}` |
| `"danger-full-access"` | `{"type": "dangerFullAccess"}` |
| `"never"` | `{"type": "never"}` |

---

## 超时机制

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `codex.turn_timeout_ms` | 3,600,000ms（1小时） | 单次 turn 的最大执行时间，超时返回 `TurnTimeout` 错误 |
| `codex.read_timeout_ms` | 5,000ms（5秒） | 握手阶段等待响应的超时时间 |
| `codex.stall_timeout_ms` | 300,000ms（5分钟） | 无事件活动的停滞检测超时（由 orchestrator 检测） |

超时行为：
- `turn_timeout_ms` 超时：返回 `CodexError::TurnTimeout`，orchestrator 将此次 turn 标记为失败并触发重试
- `read_timeout_ms` 超时（握手阶段）：返回 `CodexError::ResponseTimeout`，整个 CodexClient 启动失败
- `stall_timeout_ms` 超时：由 orchestrator 的 stall 检测逻辑处理，取消当前 turn

---

## 错误处理

### MAX_LINE_SIZE

单行 JSON 最大长度为 **10MB**（`MAX_LINE_SIZE = 10 * 1024 * 1024`）。超过此长度的行会被丢弃并记录警告日志，不会终止 turn。

### 进程退出码含义

| 情况 | 处理方式 |
|------|----------|
| stdout EOF（进程正常退出） | 返回 `CodexError::ProcessExit { code: Some(0) }` |
| stdout EOF（进程异常退出） | 返回 `CodexError::ProcessExit { code: Some(非零) }` |
| stdout I/O 错误 | 返回 `CodexError::ProcessExit { code: None }` |
| 进程未找到（NotFound） | 返回 `CodexError::NotFound` |

### CodexError 枚举

| 错误 | 说明 |
|------|------|
| `NotFound` | codex 命令不存在 |
| `InvalidWorkspaceCwd` | 工作区路径不存在 |
| `ResponseTimeout` | 握手阶段响应超时 |
| `TurnTimeout` | Turn 执行超时 |
| `ProcessExit { code }` | 进程意外退出 |
| `ResponseError { detail }` | JSON-RPC 错误响应 |
| `TurnFailed { reason }` | Turn 失败（有原因） |
| `TurnCancelled` | Turn 被取消 |
| `TurnInputRequired` | Turn 需要用户输入 |
| `MalformedMessage { raw }` | 无法解析的 JSON 行 |
| `Io(e)` | I/O 错误 |
| `Json(e)` | JSON 序列化错误 |

### Drop 行为

`CodexClient` 实现了 `Drop` trait，在对象销毁时向进程组发送 SIGKILL，确保不留下僵尸进程。
