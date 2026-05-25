# rust-platform 架构文档

## 1. 概述

rust-platform 是 Symphony 的编排运行时，单二进制 CLI 应用，基于 Tokio 异步运行时构建，采用事件驱动的单线程状态机架构。它负责持续轮询 Issue Tracker、调度 Codex Agent 子进程、管理隔离工作空间，并通过 HTTP API 暴露运行时状态。

更多背景请参阅 [`../architecture.md`](../architecture.md)。

## 2. 分层架构

```
┌─────────────────────────────────────────────────────┐
│  Policy Layer                                        │
│  WORKFLOW.md（团队策略，YAML Front Matter + Liquid）  │
└────────────────────────┬────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────┐
│  Configuration Layer                                 │
│  config/（解析、校验、热重载、ArcSwap）               │
└────────────────────────┬────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────┐
│  Coordination Layer                                  │
│  orchestrator/（调度、协调、重试、事件循环）           │
└──────────┬─────────────────────────┬────────────────┘
           │                         │
┌──────────▼──────────┐   ┌──────────▼──────────────┐
│  Execution Layer    │   │  Integration Layer       │
│  workspace/ agent/  │   │  tracker/ platform/      │
│  工作空间 + Agent   │   │  外部平台适配             │
└─────────────────────┘   └──────────────────────────┘
           │
┌──────────▼──────────────────────────────────────────┐
│  Observability Layer                                 │
│  logging/ server/（结构化日志 + HTTP Dashboard API） │
└─────────────────────────────────────────────────────┘
```

### 层次职责

| 层 | 职责 | 主要模块 |
|----|------|----------|
| Policy Layer | 团队策略定义，版本管理 | `WORKFLOW.md` |
| Configuration Layer | 配置解析、类型化、校验、热重载 | `config/workflow_loader`, `config/service_config`, `config/watcher`, `config/validator` |
| Coordination Layer | 调度、并发控制、重试、协调 | `orchestrator/mod.rs`, `orchestrator/scheduler.rs`, `orchestrator/reconciler.rs`, `orchestrator/retry.rs` |
| Execution Layer | 工作空间生命周期、Agent 进程管理 | `workspace/`, `agent/runner.rs`, `agent/codex_client.rs` |
| Integration Layer | 外部平台适配（Tracker + Platform） | `tracker/`, `platform/` |
| Observability Layer | 结构化日志、HTTP API | `logging/`, `server/api.rs` |

各层单向依赖，不存在循环引用：`Policy → Configuration → Coordination → Execution → Integration`。

## 3. 事件驱动模型

编排器采用单线程事件循环，所有状态变更通过单一 `mpsc` channel 序列化处理，彻底消除锁竞争和数据竞态。

### OrchestratorEvent 枚举

| 事件 | 来源 | 处理逻辑 |
|------|------|----------|
| `Tick` | 定时器（按轮询间隔触发） | 触发 Tracker 轮询、协调、调度 |
| `WorkerExitNormal` | Worker Tokio 任务 | 短延迟后续行调度（Continuation） |
| `WorkerExitAbnormal` | Worker Tokio 任务 | 指数退避重试，加入重试队列 |
| `CodexUpdate` | Agent 事件流（Codex stdout） | 更新 LiveSession token 计数和活动时间戳 |
| `RetryFired` | 重试定时器 | 将 Issue 重新加入调度候选 |
| `ConfigReloaded` | 文件监听器（notify） | ArcSwap 原子替换配置，下次 Tick 生效 |
| `ForceRefresh` | HTTP API（POST /api/v1/refresh） | 立即触发一次 Tracker 轮询 |
| `QueryState` | HTTP API（GET /api/v1/state） | 构建状态快照，通过 oneshot channel 回复 |
| `QueryIssue` | HTTP API（GET /api/v1/:id） | 查询单个 Issue 详情，通过 oneshot channel 回复 |
| `Shutdown` | 信号处理器（SIGINT/SIGTERM） | 优雅关闭：停止调度 → 等待 Worker 完成 → 退出 |

### 事件循环主流程

```
loop {
    select! {
        event = rx.recv()  => handle_event(event),
        _ = tick_interval  => send(Tick),
    }
}

handle_event(Tick):
    1. poll_tracker()          // 拉取活跃 Issue
    2. reconcile()             // 停滞检测 + 终态协调
    3. schedule()              // 调度新任务
```

## 4. 核心模块

### 4.1 orchestrator/mod.rs — 主循环

编排器的入口，持有所有运行时状态（`OrchestratorState`），驱动事件循环。负责：
- 接收并分发所有 `OrchestratorEvent`
- 维护 `running`、`claimed`、`retry_attempts`、`completed` 等状态集合
- 协调 Scheduler、Reconciler、Retry 三个子模块

### 4.2 orchestrator/scheduler.rs — 调度器

从 Tracker 拉取的候选 Issue 中，按策略选择可执行的 Issue 并启动 Worker：

```
候选 Issue 列表
    → 过滤已 claimed（running + retrying）
    → 过滤已 completed
    → 检查阻塞依赖（blocked_by）
    → 检查全局并发上限（max_concurrent_agents）
    → 检查按状态并发上限（max_concurrent_agents_by_state）
    → 按优先级 + 创建时间排序
    → 取前 N 个，spawn Worker 任务
```

### 4.3 orchestrator/reconciler.rs — 协调器

在每次 Tick 时对运行中的 Worker 进行双阶段协调：

**Part A — 停滞检测**：基于单调时钟检测无活动的 Agent。若 Agent 超过 `stall_timeout` 未产生任何 Codex 事件，则终止并触发重试。

**Part B — 终态协调**：查询 Tracker 确认运行中 Issue 的当前状态。若 Issue 已进入终态（非 `active_states`）或从 Tracker 消失，则取消对应 Worker，释放资源。

> 注意：状态比较必须使用 `normalize_state()`，确保与 `normalize_tracker_state()` 行为一致（trim + lowercase + 空格/连字符转下划线）。详见 `CLAUDE.md` 中的状态归一化规范。

### 4.4 orchestrator/retry.rs — 重试模块

管理异常退出 Issue 的重试逻辑：
- 指数退避计算：`delay = base * 2^attempts`，上限为 `max_backoff`（默认 5 分钟）
- 通过 `tokio::time::sleep` + `RetryFired` 事件实现非阻塞延迟重试
- `retry_attempts` 计数持久化在 `OrchestratorState` 中

## 5. 状态模型

`OrchestratorState` 是编排器的核心状态，仅在事件循环内访问，无需加锁：

```rust
struct OrchestratorState {
    // 运行中的 Worker：issue_id → RunningEntry
    running: HashMap<String, RunningEntry>,

    // 已认领的 Issue 集合（running + retrying），防止重复调度
    claimed: HashSet<String>,

    // 重试状态：issue_id → RetryEntry
    retry_attempts: HashMap<String, RetryEntry>,

    // 已完成的 Issue（含完成时间戳，用于 GC）
    // GC 策略：完成超过 1 小时的记录自动清除
    completed: HashMap<String, Instant>,

    // Codex 全局 token 统计
    codex_totals: CodexTotals,

    // 其他运行时状态
    poll_interval_ms: u64,
    max_concurrent_agents: usize,
    codex_rate_limits: ...,
    shutting_down: bool,
}
```

`RunningEntry` 包含单个运行中 Worker 的完整信息：
- `issue`：Issue 元数据快照
- `live_session`：`LiveSession` 实时会话数据
- `started_at`：启动时间
- `cancel_token`：`CancellationToken`，用于优雅取消
- `abort_handle`：Tokio task 的 abort handle

`LiveSession` 记录 Codex 会话的实时状态：
- `session_id`、`thread_id`、`turn_id`：会话标识
- `codex_app_server_pid`：Codex 子进程 PID
- `input_tokens`、`output_tokens`、`total_tokens`：token 计数
- `last_activity_instant`：最后活动时间（单调时钟，用于停滞检测）

## 6. 配置热重载

```
WORKFLOW.md 文件变更
    │
    │ notify crate（跨平台文件系统事件）
    ▼
解析新配置（workflow_loader）
    │
    ▼
校验配置完整性（validator）
    │
    ▼
ArcSwap 原子替换 ConfigHolder
    │
    ▼
发送 ConfigReloaded 事件 → Orchestrator
    │
    ▼
下次 Tick 使用新配置（轮询间隔、并发限制等立即生效）
```

使用 `arc-swap` 实现无锁配置读取：配置消费者（Scheduler、Reconciler）通过 `config.load()` 获取当前配置快照，始终读到一致的版本，无需加锁。

## 7. 扩展点

| 扩展方向 | 实现方式 | 说明 |
|----------|----------|------|
| 新增 Tracker | 实现 `Tracker` trait | 需实现 `fetch_candidate_issues`、`fetch_issues_by_states`、`fetch_issue_states_by_ids` |
| 新增 Platform | 实现 `Platform` trait | 需实现 Issue CRUD、Label 管理、PR 创建等接口 |
| 新增 Agent 后端 | 替换 `agent/codex_client.rs` | 当前协议为 JSON-line stdio |
| 自定义调度策略 | 修改 `orchestrator/scheduler.rs` | 优先级算法、阻塞检查逻辑 |
| 自定义 Hook | 在 WORKFLOW.md 中配置 shell 脚本 | `after_create`、`before_run`、`after_run`、`before_remove` |
| 自定义 Prompt | 修改 WORKFLOW.md 模板正文 | Liquid 模板语法，支持首轮/续行模板 |
| 新增 Agent 工具 | 实现 `DynamicTool` trait | 注册到 `tools/` 模块 |

## 8. 模块依赖图

```
main.rs
├── cli.rs（Clap 参数定义）
├── config/
│   ├── workflow_loader（WORKFLOW.md 解析）
│   ├── service_config（类型化配置）
│   ├── watcher（热重载，notify + ArcSwap）
│   ├── validator（配置校验）
│   └── platform（平台配置结构）
├── orchestrator/
│   ├── mod.rs（主循环，OrchestratorState）
│   ├── scheduler.rs（调度逻辑）
│   ├── reconciler.rs（协调逻辑）
│   └── retry.rs（重试计算）
├── agent/
│   ├── runner.rs（Worker 生命周期）
│   └── codex_client.rs（Codex JSON-line 协议）
├── tracker/
│   ├── linear.rs（Linear GraphQL）
│   └── gitlab.rs（GitLab 桥接）
├── platform/
│   ├── github.rs（GitHub REST）
│   ├── gitlab.rs（GitLab REST）
│   ├── http_client.rs（带重试的 HTTP）
│   ├── cooldown_queue.rs（限流队列）
│   ├── issue.rs（标准化 Issue 类型）
│   ├── workflow.rs（状态机管理）
│   ├── memory.rs（内存测试适配器）
│   └── retry.rs（HTTP 重试逻辑）
├── workspace/（目录生命周期管理）
├── prompt/（Liquid 模板渲染）
├── server/
│   └── api.rs（Axum 路由，HTTP Dashboard）
├── tools/
│   ├── linear_graphql.rs（Agent 工具）
│   └── platform_api.rs（Agent 工具）
├── models/（领域模型，含 normalize_state）
├── proxy.rs（代理配置，ProxyMode 枚举）
├── error.rs（类型化错误，thiserror）
└── logging/（tracing-subscriber 初始化）
```
