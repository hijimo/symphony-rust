# Orchestrator 实现细节

源文件：`rust-platform/src/orchestrator/`

---

## 事件循环

Orchestrator 采用单线程事件循环模式，所有状态变更通过 `mpsc` channel 序列化，避免锁竞争。

```rust
// 核心循环结构（orchestrator/mod.rs）
loop {
    tokio::select! {
        _ = self.cancel.cancelled() => {
            self.handle_shutdown().await;
            break;
        }
        event = self.event_rx.recv() => {
            match event {
                Some(OrchestratorEvent::Shutdown) => {
                    self.handle_shutdown().await;
                    break;
                }
                Some(evt) => self.handle_event(evt).await,
                None => break,  // channel 关闭
            }
        }
    }
}
```

`tokio::select!` 同时监听：

- `cancel.cancelled()` — 外部取消信号（SIGINT/SIGTERM）
- `event_rx.recv()` — 来自 worker、HTTP API、定时器的事件

---

## 事件处理

所有事件通过 `handle_event` 分发：

| 事件 | 来源 | 处理函数 |
|------|------|----------|
| `Tick` | 定时器 | `on_tick()` — 触发轮询、协调、调度 |
| `WorkerExitNormal` | Worker 任务 | `on_worker_exit_normal()` — 续行调度 |
| `WorkerExitAbnormal` | Worker 任务 | `on_worker_exit_abnormal()` — 指数退避重试 |
| `CodexUpdate` | Agent 事件流 | `on_codex_update()` — 更新 Token 计数、活动时间 |
| `RetryFired` | 重试定时器 | `on_retry_fired()` — 重新调度 Issue |
| `ConfigReloaded` | 文件监听器 | `on_config_reloaded()` — 更新运行时参数 |
| `ForceRefresh` | HTTP API | 直接调用 `on_tick()` |
| `QueryState` | HTTP API | 构建并返回状态快照 |
| `QueryIssue` | HTTP API | 返回单个 Issue 详情 |
| `Shutdown` | 信号处理器 | `handle_shutdown()` |

---

## 调度算法

每次 `Tick` 事件触发完整的调度流程（`on_tick` → `dispatch_and_spawn`）：

```
1. GC 已完成记录（清理 completed 集合中的过期条目）
2. 协调器 Part A：停滞检测（见下文）
3. 协调器 Part B：终态协调（见下文）
4. 调度预检（preflight）：确认 tracker 已配置
5. fetch_candidate_issues() → Vec<TrackerIssue>
6. 转换为 Vec<Issue>
7. sort_for_dispatch()：按优先级排序（priority 数值越小越优先，None 排最后）
8. 遍历候选 Issue：
   a. 检查全局并发槽位（has_global_slots）
   b. should_dispatch() 检查：
      - 未在 running 集合中
      - 未在 claimed 集合中（running + retrying）
      - 未在 completed 集合中（近期完成的去重）
      - 状态在 active_states 中
      - 未被阻塞（blocked_by 中的依赖未完成）
      - per-state 并发未超限
   c. 通过 tracker 重新验证 Issue 状态（防止 stale dispatch）
   d. 加入 claimed 集合，spawn worker
9. 调度下一次 Tick 定时器
```

---

## 并发控制

三层并发控制机制：

**全局并发上限**（`max_concurrent_agents`）

```rust
fn has_global_slots(&self) -> bool {
    self.running.len() < self.max_concurrent_agents
}
```

**按状态并发上限**（`max_concurrent_agents_by_state`）

在 `should_dispatch` 中检查：当前该状态的运行数是否已达到配置的上限。

**Claimed 集合去重**

`claimed` 集合包含所有 running + retrying 的 Issue ID，防止同一 Issue 被重复调度：

```rust
if self.state.claimed.contains(&issue.id) {
    continue;  // 已在处理中，跳过
}
```

---

## 停滞检测

**Part A — 停滞检测**（`reconcile_stalled_runs`，`orchestrator/reconciler.rs`）

使用单调时钟（`Instant::now()`）避免 NTP 时间漂移影响：

```
对每个 running entry：
    elapsed = Instant::now() - entry.session.last_activity
    if elapsed > stall_timeout_ms:
        发送 CancellationToken（触发 after_run hook）
        记录 cancel_sent_at
        if cancel_sent_at 超过 30s 仍未退出:
            force-kill（abort worker task）
            加入重试队列（Failure 类型）
```

`last_activity` 在每次收到 `CodexUpdate` 事件时更新（`entry.session.touch()`）。

---

## 协调器两阶段

**Part A — 停滞检测**（每次 Tick 执行）

1. 检测无活动超时的 worker
2. 发送取消信号，等待 30 秒
3. 超时后强制 kill，加入 Failure 重试队列

**Part B — 终态协调**（每次 Tick 执行，需要 tracker）

1. 收集所有 running Issue 的 ID
2. 批量查询 tracker 获取最新状态（`fetch_issue_states_by_ids`）
3. 对每个 Issue 判断：
   - 状态在 `terminal_states` 中 → `TerminateAndClean`（终止 worker + 清理工作空间）
   - 状态不在 `active_states` 中 → `TerminateNoClean`（终止 worker，保留工作空间）
   - 状态仍在 `active_states` 中 → 继续运行

---

## 重试策略

**Continuation 重试**（worker 正常退出但 Issue 仍活跃）

- 延迟：固定 1 秒
- 触发：`WorkerExitNormal` 且 Issue 状态仍在 `active_states`

**Failure 重试**（worker 异常退出或停滞超时）

- 延迟：`10s * 2^(attempt-1)`，上限为 `max_retry_backoff_ms`
- 示例（默认上限 300s）：10s → 20s → 40s → 80s → 160s → 300s → 300s → ...
- 触发：`WorkerExitAbnormal` 或停滞 hard deadline 超时

重试时重新从 tracker 获取最新 Issue 状态，若已进入终态则放弃重试。

---

## Worker 生命周期

```
spawn_worker_with_attempt(issue, retry_attempt)
    │
    ├── 构建 AgentIssue（从 Issue 转换）
    ├── 克隆 prompt_engine、workspace_mgr、config_holder、tracker
    ├── tokio::spawn 异步任务：
    │       ├── 创建 codex_tx/codex_rx channel（转发 CodexUpdate 事件）
    │       ├── 构建 AgentRunner
    │       ├── 构建 TrackerStateRefresher（桥接 Tracker → IssueStateRefresher）
    │       └── runner.run_attempt(agent_issue, retry_attempt, state_refresher)
    │               ├── 正常退出 → 发送 WorkerExitNormal 事件
    │               └── 异常退出 → 发送 WorkerExitAbnormal 事件
    │
    └── 注册 RunningEntry（含 worker_handle、cancel_token、session、started_at）
```

---

## 配置热重载影响

`on_config_reloaded` 处理器仅更新 orchestrator 的运行时参数：

```rust
fn on_config_reloaded(&mut self) {
    self.state.poll_interval_ms = self.dispatch_config.poll_interval_ms;
    self.state.max_concurrent_agents = self.dispatch_config.max_concurrent_agents;
}
```

- 已运行的 worker 持有启动时的配置快照（`config_holder.snapshot()`），不受影响
- 新调度的 worker 使用最新配置
- `dispatch_config`（active_states、terminal_states 等）在 `main.rs` 中从 `ConfigHolder` 读取，热重载后新 Tick 会使用更新后的值

---

## 关键数据结构

```rust
// 运行中的 worker 条目
struct RunningEntry {
    worker_handle: JoinHandle<()>,    // Tokio 任务句柄
    cancel_token: CancellationToken,  // 取消信号
    identifier: String,               // Issue 人类可读 ID
    issue: Issue,                     // Issue 快照
    session: LiveSession,             // Codex 会话状态（token 计数、活动时间）
    retry_attempt: Option<u32>,       // 重试次数
    started_at: Instant,              // 单调时钟（用于运行时统计）
    started_at_utc: DateTime<Utc>,    // UTC 时间（用于 API 响应）
    cancel_sent_at: Option<Instant>,  // 取消信号发送时间（用于 hard deadline）
}

// 重试队列条目
struct RetryEntry {
    issue_id: String,
    identifier: String,
    attempt: u32,
    retry_kind: RetryKind,            // Continuation | Failure
    due_at_ms: u64,                   // 单调时钟毫秒（到期时间）
    timer_handle: JoinHandle<()>,     // 定时器任务句柄
    error: Option<String>,            // 上次失败原因
}
```
