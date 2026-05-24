# Symphony 续跑与恢复架构方案草案

## 背景

当前 Symphony 的 GitHub/GitLab 编排依赖 issue label 发现待执行任务，并用 per-issue workspace 与 `## Codex Workpad` 评论保存 agent 执行上下文。这个模型的优点是简单：tracker 上的状态对人和系统都可见，agent 可以通过 workpad 续接自己的计划和验证记录。

真实运行暴露出三个恢复缺口：

- 同进程 retry 中，orchestrator 已经计算 retry attempt，但没有传给 `AgentRunner` 和 prompt，导致模板中的 continuation context 不生效。
- 服务停止、Web 平台崩溃、机器重启后，rust-platform 内存里的 running/retrying/claimed/timer 全部丢失。
- workspace 目录只要存在就会被复用；如果上一次 `after_create` 中途失败，只留下空目录、半初始化 Git 仓库或错误目录结构，后续运行会继续在坏 workspace 上执行。

本方案目标是在保持现有“tracker label 是调度资格事实来源”的前提下，补齐 retry continuation、进程级恢复、workspace 初始化完整性和重复执行防护。

## 修订结论

目标架构采用 **A + C + Rust-side B-lite**：

1. **A：retry attempt 贯通**
   同进程 retry 必须把 `retry_attempt` 传入 prompt。

2. **C：workspace metadata、健康检查和锁**
   workspace 自己持有初始化健康状态，坏 workspace 不进入 Codex run。

3. **Rust-side B-lite：rust-platform 本地 resume snapshot**
   rust-platform 在 workspace root 下写本地恢复提示文件，第一阶段仅用于诊断和 prompt hint，不参与调度 gate，不保存或恢复 retry due_at。

第一阶段不在 Web SQLite 中新增 issue 级恢复摘要表。Web 平台只负责项目级进程生命周期：启动、停止、PID/process group 管理、auto-restart、日志和服务状态。issue 级 running/retrying/attempt 是 rust-platform 的领域，不能由 Web 平台推测。

### 第一阶段 MVP 范围

第一阶段不一次性实现完整目标架构，只交付能消除当前重复执行和半初始化风险的最小闭环：

1. **retry correctness**
   - 贯通同进程 `retry_attempt`。
   - 所有 spawn 前 revalidation 必须确认 fresh tracker state 仍在 `active_states`。
   - `NoSlot`/无并发槽延迟必须独立于 failure retry，不增加 prompt attempt。
   - 删除或禁用 orchestrator 写 workflow label 的路径。

2. **Web process uniqueness**
   - stop/restart/startup cleanup/Web SIGTERM 按 service session/tree 清理，而不只杀 Symphony PID。
   - watcher auto-restart 必须走 DB lifecycle fencing/CAS，并在 backoff 后重新读取 DB/ProcessManager generation。

3. **workspace minimal ready manifest + live writer lock**
   - 只保证没有 ready metadata 的目录不进入 Codex。
   - lock 覆盖整个 worker run，不只覆盖初始化。

4. **resume JSON hint**
   - 只保存 last_error、last_lifecycle、last_run_id、resume hint。
   - 不恢复完整状态机，不写 Web SQLite issue 级表，不参与第一阶段调度决策。

## 状态源契约

| 状态源 | 负责内容 | 不负责内容 | 写入方 |
|---|---|---|---|
| Tracker workflow label | issue 是否具备调度资格：`active_states` 可调度，terminal 可清理，其他状态不调度 | 不表达 retry timer、不表达 workspace 健康、不表达 agent 计划完成度 | Agent/人工；orchestrator 只读 |
| Orchestrator memory | 当前进程内的 claimed/running/retrying、并发槽、retry timer、stall detection | 不跨进程持久化、不作为重启后唯一事实 | rust-platform |
| Rust resume snapshot | 最近一次 issue 运行快照、last_error、diagnostic_resume_hint、诊断 | 第一阶段不表达 dispatch gate、不恢复 retry timer、不替代 tracker label、不替代 workspace metadata | rust-platform |
| Workspace metadata | workspace 是否初始化完成、是否可复用、初始化错误 | 不决定 issue 是否 active、不记录 tracker state | rust-platform workspace manager |
| Workpad comment | agent 计划、acceptance criteria、validation、人工可读进度 | orchestrator 不解析，不作为调度条件 | Agent |
| Web service state | 项目级服务状态、PID/process group、restart_count、log tail | 不推测 per-issue running/retrying 状态 | web-platform |

关键约束：

- 是否启动 worker 的正向资格只来自 tracker 当前状态、orchestrator 当前并发/claim、workspace 可用性。
- `resume snapshot` 第一阶段只能增强 prompt 和诊断；它不是 positive scheduling source，也不是 negative scheduling gate。
- `workpad` 只能由 agent 查找和 reconcile；服务端不解析 workpad。
- `workspace metadata` 是 workspace 健康唯一事实来源；snapshot 只能缓存最近错误，不能覆盖 metadata。

## 术语

- `retry_attempt`：同一 rust-platform 进程内 failure retry 或 continuation retry 的 prompt attempt 计数。
- `backoff_attempt`：同进程内 retry/backoff 序列计数，用于计算 bounded delay，不直接伪装成 prompt retry；第一阶段不跨重启恢复。
- `queue_delay_count`：无并发槽导致的排队延迟次数，不属于失败重试。
- `resume_reason`：非同进程 retry 的保守提示，例如 `service_restart`、`crash_recovery`、`workspace_recovery`、`active_resume`、`unknown_resume`。第一阶段不承诺精确识别 manual requeue。
- `run_id`：一次 rust-platform 子进程运行的唯一 ID。
- `service_instance_id`：Web 平台启动一次项目服务生成的实例 ID，用于 fencing 旧进程。
- `issue_id_path_key`：issue id 的 path-safe 编码，用于所有文件名级 issue identity。第一阶段固定采用 `i-` + lowercase_hex(utf8(raw_issue_id))，保证在大小写不敏感文件系统上也不碰撞；不得使用 base64/base64url 这类含大小写双字母表的编码，不能用短 hash，不能假设 tracker id 天然适合作为文件名。若未来因路径长度改用 hash，必须使用 `h-` + full SHA-256 lowercase hex，并在 metadata 中保存 raw `issue_id` 强制校验。
- `workspace_key`：由 `issue_id_path_key` 加 issue identifier sanitize 后得到的可读 workspace 目录名，格式为 `<issue_id_path_key>-<sanitized_identifier>`。`sanitized_identifier` 只用于可读性，不能单独作为身份，不能参与 canonical lock 身份。
- `legacy_workspace_key`：旧版本仅由 issue identifier sanitize 得到的目录名。升级到新 key 前必须显式探测旧目录。

`retry_attempt`、`backoff_attempt`、`resume_reason` 必须分开。服务重启、机器重启、人工把 issue 改回 `Todo`，不应伪装成同进程 retry attempt。第一阶段不跨重启延续 backoff；prompt 中使用 `resume_reason` 表达恢复原因。

## 设计目标

1. 同进程 retry prompt 能明确收到 `retry_attempt`。
2. 重启后 active issue 可以通过 tracker label 被重新发现，并通过 workspace/workpad/resume snapshot 获得 continuation 语义。
3. 所有 spawn 前 fresh tracker state 都必须仍在 `active_states`；非 active 且非 terminal 的 issue 不再被 retry fired 或普通 poll 重新 spawn。
4. 半初始化 workspace 不被静默复用。
5. 同一 issue workspace 在旧进程未退出时不会被新进程同时写入。
6. Web 平台不会因为 watcher auto-restart、手动 stop/start、SIGTERM 竞态启动两个同项目子进程。
7. 恢复失败时进入可诊断状态，而不是无限 retry 或静默清理。

## 非目标

- 不做完整 orchestrator 状态机持久化。
- 不恢复 retry timer 的精确剩余时间到毫秒级。
- 不让 Web SQLite 成为 issue 级运行状态事实来源。
- 不让服务端解析或修改 workpad。
- 不自动恢复非 active issue；这类 issue 需要人工重新入队或人工确认。

## 当前问题

### 1. retry attempt 没有贯通

`AgentRunner::run_attempt` 已接收 `attempt: Option<u32>` 并传给 `PromptEngine::render`，但 orchestrator 启动 worker 时固定传 `None`。因此 retry 逻辑存在于内存队列，但 prompt 永远看不到 `{% if attempt %}`。

必须修复：

- MVP：`spawn_worker(issue, retry_attempt: Option<u32>, run_context: RunContext)`。
- `RunContext` 第一阶段必须符合下文固定 ABI，包含 `resume_reason`、`workspace_ready` 和 snapshot hint。
- 初次 poll candidate 调度传 `None`。
- failure retry、continuation retry 传对应 `Some(entry.attempt)`。
- no-slot reschedule 不增加用户可见 `retry_attempt`，使用单独的 `queue_delay_count`；来自 retry/continuation 的 no-slot 必须保留原 attempt。
- tracker fetch/revalidation 失败属于 pre-spawn delay，不属于 worker failure；不得增加用户可见 `retry_attempt`，只能增加独立的 `revalidation_delay_count` 或 `backoff_attempt`。
- `RunningEntry.retry_attempt` 保存同一个值。
- 日志和 API 状态显示 attempt。
- retry worker 的第一 turn 仍是新的 Codex turn，不能假设会自动使用 continuation template。MVP 必须二选一：
  - 在 main template 中显式处理 `attempt`/`resume_reason`。
  - 或让 `PromptEngine` 在 `attempt.is_some()`/`resume_reason.is_some()` 时选择 retry/resume template。

### 2. retry fired 只检查 terminal，不检查 active

当前 retry fired 后重新 fetch issue，只检查 terminal。如果 issue 已被 agent 或人工移到 `Human Review` 这类非 active 且非 terminal 状态，仍有机会重新 spawn worker。

必须修复：

- retry fired 后必须检查 tracker 当前状态：
  - terminal：释放 claim，必要时清理 workspace。
  - active：可调度，进入 slot/workspace 检查。
  - 非 active 且非 terminal：释放 claim，保留诊断，不 spawn worker。

### 3. 服务停止/崩溃后内存状态丢失

`running`、`retry_attempts`、timer、claimed 都在 rust-platform 内存里。进程结束后只能靠 tracker active label 重新发现任务。

新的策略：

- 继续承认这个事实，不尝试恢复完整内存状态机。
- 第一阶段 rust-platform 写本地 per-issue resume JSON，仅记录最近运行快照、last_error、last_run_id 和 resume hint。
- 重启后 active issue 仍由 tracker 发现；snapshot 只提供 `resume_reason`、last_error 等提示，不决定是否调度。

### 4. 半初始化 workspace 被复用

workspace manager 当前只判断目录是否存在；存在就不跑 `after_create`。这会复用空目录、半 clone 仓库或错误布局。

必须修复：

- workspace metadata 原子写。
- workspace 健康检查。
- workspace lock/fencing。
- 坏 workspace 不能进入 Codex app-server。

### 5. Web 进程生命周期存在重复执行风险

Web 平台启动子进程时用了 `setsid()`，但停止和清理必须按 process group 处理，否则 Codex 子进程可能残留。watcher auto-restart 也必须和手动 start/stop 共用 per-project lock，否则可能产生两个 Symphony 实例。

## 方案 A：retry attempt 贯通

### 接口变化

MVP 接口：

```rust
fn spawn_worker(
    &mut self,
    issue: Issue,
    retry_attempt: Option<u32>,
    run_context: RunContext,
)
```

`AgentRunner::run_attempt` 第一阶段也必须接收 `RunContext` 或等价 prompt context。MVP 必须保证 retry/resume worker 的第一 turn 能在 prompt 中看到 retry/resume 信息：要么 main template 读取 `attempt` 和 `resume_reason`，要么 `PromptEngine` 基于 `attempt`/`run_context` 选择 retry/resume template。

### RunContext prompt ABI

第一阶段固定 `RunContext` ABI，避免模板和实现各自解释：

```rust
pub struct RunContext {
    pub resume_reason: Option<ResumeReason>,
    pub workspace_ready: bool,
    pub snapshot_hint: Option<SnapshotHint>,
}

pub enum ResumeReason {
    ServiceRestart,
    CrashRecovery,
    WorkspaceRecovery,
    ActiveResume,
    UnknownResume,
}

pub struct SnapshotHint {
    pub last_lifecycle: Option<String>,
    pub last_error_code: Option<String>,
}
```

Liquid 变量名固定为顶层兼容变量：

- `attempt`: `Option<u32>`，只表示同进程 retry/continuation attempt。
- `resume_reason`: `Option<String>`，取值为 `service_restart`、`crash_recovery`、`workspace_recovery`、`active_resume`、`unknown_resume`。
- `workspace_ready`: `bool`，进入 Codex 的 prompt 中必须为 `true`。
- `snapshot_hint.last_lifecycle`、`snapshot_hint.last_error_code`: 可选诊断字段。

非 ready workspace 状态（例如 `failed`、`legacy_requires_manual`、`identity_mismatch`、`locked_by_live_process`）只进入日志/API/snapshot 诊断，不能进入 Codex prompt ABI。

空值语义：

- `resume_reason=None` 表示普通首次调度，不渲染恢复说明。
- `snapshot_hint=None` 表示没有可用诊断 snapshot，不影响调度。
- 模板不得把 `snapshot_hint` 当成事实源；只能把 `resume_reason` 作为人类可读恢复提示。

模板选择规则：

- 第一阶段推荐保持 main template 兼容，并在 main template 中读取上述顶层变量。
- 如果新增 retry/resume template，选择优先级必须是：`attempt.is_some()` 或 `resume_reason.is_some()` 优先于 `turn_number > 1`；并保留旧模板变量兼容。
- 验收必须渲染 main template 和 retry/resume template 两种路径，证明第一 turn 能看到 `attempt`/`resume_reason`。

### retry 类型

- `RetryKind::Failure`：worker abnormal exit，指数 backoff。
- `RetryKind::Continuation`：worker normal exit 但 issue 仍 active，固定短 delay。
- `RetryKind::NoSlot` 或等价 queue-delay 状态：无并发槽，使用 bounded delay，不伪装成 failure，不增加 prompt `retry_attempt`。

MVP 必须把 no-slot 从 failure retry 中拆出来：可以新增 `RetryKind::NoSlot`，也可以使用独立 queue-delay entry；但不得复用 `RetryKind::Failure` 或递增 prompt attempt。

### 验收

- failure retry 第二次运行 prompt 包含 `attempt`。
- continuation retry 第二次运行 prompt 包含 `attempt`。
- no-slot reschedule 不增加 prompt retry attempt，且不会被用户误读为失败重试。
- 日志 `starting agent run attempt` 不再在 retry 时显示 `None`。
- retry worker 第一 turn 的 prompt 实际包含 retry/resume 信息，而不是只把 `attempt` 写进未被使用的 continuation template。

### 全局 spawn 前 revalidation

所有 worker spawn 路径都必须执行同一套 fresh-state revalidation：

- 普通 poll candidate dispatch。
- retry fired。
- continuation retry。
- integration-layer dispatch/register 入口。

规则：

- fresh state 在 `active_states`：继续 slot/workspace 检查。
- fresh state 在 terminal states：释放 claim，清理 retry/snapshot。
- fresh state 非 active 且非 terminal：释放 claim，保留诊断，不 spawn。
- tracker fetch/revalidation unavailable：不 spawn；首次 poll candidate 不 claim 或释放 claim，等待下一轮 poll；retry/continuation entry 必须保留原 `retry_attempt`、`RetryKind`、`RunContext`，只允许更新独立 `revalidation_delay_count`/`backoff_attempt` 和诊断错误。

normal worker exit 后不得直接 schedule continuation retry。continuation schedule 决策必须由 orchestrator event loop 在处理 `WorkerExit` 时通过同一个 `ReadOnlyTracker.refresh(issue_id)` 获取 fresh issue/state 后做出；`WorkerExitNormal` 最多携带 worker 退出诊断和最后观察到的 tracker state，不能携带可作为调度依据的 `still_active`，也不能让 worker/task 自己写 continuation retry。只有 event loop 刷新的 fresh state 仍在 `active_states` 时才允许写 `active_after_normal_exit` 诊断并 schedule continuation retry。

`spawn_worker` 必须使用最后一次 revalidation 返回的 fresh `Issue` 对象启动 runner；旧 `Issue` 只能用于 stable identity、日志和 refresh key。`attempt` 从 `1` 开始，`Some(0)` 非法并应在构造 retry entry 时拒绝。`attempt.is_some()` 和 `resume_reason.is_some()` 同时存在时，模板必须同时暴露两者；模板选择优先级仍以 retry attempt 优先，但不得丢弃 `resume_reason`。

旧的 `dispatch_candidates`、`register_running` 等 integration-layer 入口必须删除、私有化，或强制路由到同一 `spawn_worker(issue, retry_attempt, run_context)` gate；不得保留绕过 revalidation、workspace eligibility、canonical lock 的登记路径。

### Tracker 只读边界 ABI

第一阶段不能只删除当前 `set_workflow_state` 调用点，还必须把类型边界改成不可写：

- Orchestrator 只持有 `ReadOnlyTracker` 或等价只读 trait，能力仅包含候选查询、按 id 刷新 issue state、读取 blocker 等调度所需方法。
- workflow label mutation 必须放入独立 `WorkflowMutator`/agent 平台写接口，由 agent/人工路径持有；orchestrator 模块不得依赖该 trait。
- `rust-platform/src/orchestrator/**` 不得引用 `set_workflow_state` 或等价 mutation 方法。
- 如果现有 `Tracker` trait 暂时无法拆分，MVP 必须提供只读 wrapper 注入 orchestrator，并在编译边界或测试中证明 orchestrator 无法调用 label mutation。

## 方案 B-lite：rust-platform 本地 resume snapshot

第一阶段只实现诊断/prompt hint，不实现 retry due_at 恢复。`retry_due_at` 如果未来要持久化，必须另起阶段把它正式列为负向调度状态源，并定义失效、人工 override、时钟漂移和优先级。

### 文件位置

推荐使用 per-issue 文件，避免单文件多 writer 读改写覆盖：

```text
<workspace_root>/.symphony/resume/issues/<issue_id_path_key>.json
```

如果后续需要全局视图，可以由只读聚合器生成：

```text
<workspace_root>/.symphony/resume-snapshot.json
```

每个 issue 文件使用临时文件 + rename 原子写，并尽量 fsync 文件和父目录。snapshot 写入必须由 orchestrator event loop 串行完成；worker 只发事件，不直接写文件。若未来允许多写者，必须使用 `updated_seq`/`run_id` fencing 防止旧状态覆盖新状态。全局 snapshot 只能由只读聚合器生成。snapshot 不得记录 issue body、prompt、token 或命令输出中的敏感内容。

### 建议结构

```json
{
  "schema_version": 1,
  "issue_id": "6",
  "issue_id_path_key": "i-36",
  "issue_identifier": "#6",
  "workspace_key": "i-36-_6",
  "workspace_path": "/abs/path/i-36-_6",
  "run_id": "uuid",
  "service_instance_id": "uuid-or-web-provided",
  "updated_seq": 42,
  "updated_at": "2026-05-23T08:00:00Z",
  "last_observed_tracker_state": "In Progress",
  "last_lifecycle": "worker_failed",
  "last_run_id": "uuid",
  "last_prompt_retry_attempt_observed": 2,
  "diagnostic_resume_hint": "service_restart",
  "last_error": {
    "code": "codex_turn_cancelled",
    "message": "codex error: turn cancelled",
    "at": "2026-05-23T08:00:30Z"
  }
}
```

### 写入时机

由 rust-platform 的 orchestrator event loop 写入：

- worker start：`running`。
- worker abnormal exit：记录 `last_lifecycle=worker_failed`、`last_prompt_retry_attempt_observed`、`last_error`。
- worker normal exit 后 re-fetch 发现 issue 仍 active：记录 `last_lifecycle=active_after_normal_exit`，但 continuation retry 仍由内存队列控制。
- retry fired 后实际 spawn：更新 `last_lifecycle=retry_spawned`。
- issue terminal：记录 `terminal_seen` 或删除 issue 记录。
- issue 非 active 且非 terminal：记录 `stopped_unresolved`，不 spawn。
- graceful shutdown：把当前 running 标记为 `stopping`，记录 `diagnostic_resume_hint=service_restart`。

### 重启读取规则

- tracker 当前状态 active：可以进入调度候选；snapshot 只提供 resume hint 和诊断。
- tracker 当前状态 terminal：不调度，可清理 snapshot/workspace。
- tracker 当前状态非 active 且非 terminal：不调度；如果 snapshot 显示最近生命周期为 running/stopping/worker_failed，保留 `stopped_unresolved` 诊断，等待人工确认或人工改回 `Todo`。

### resume_reason 推导规则

`resume_reason` 是 prompt hint，不是调度输入。它必须由 fresh tracker state、当前服务启动方式、workspace metadata 和 snapshot 共同推导，并遵守以下优先级：

1. fresh tracker state 非 active：不启动 worker，不生成 prompt hint。
2. workspace metadata/lock 显示恢复或隔离状态：使用 `workspace_recovery` 或对应诊断，不覆盖 workspace 决策。
3. 本次 rust-platform 是 Web auto-restart 启动且上一实例异常退出：使用 `crash_recovery`。
4. 本次 rust-platform 是正常 service restart，且 snapshot `last_lifecycle=stopping` 且未过期：使用 `service_restart`。
5. 人工从非 active 改回 active 的场景第一阶段不精确识别；只有存在匹配且未过期的 snapshot/workspace metadata，或明确 startup/recovery 信号时，才可生成 `active_resume`。
6. snapshot 缺失、过期、与当前 issue identity 不匹配，且没有明确 startup/recovery 信号：使用 `unknown_resume` 或不提供 resume hint。普通首次调度允许 `resume_reason=None`。

第一阶段完全不产生 `manual_requeue`，即使 tracker API 提供 label transition token 也不进入 ABI；该值如有需要放到第二阶段设计。只靠 issue `updated_at` 不足以区分评论、标题、其他 label 更新。

snapshot staleness 规则：

- snapshot 的 `issue_id`、`issue_id_path_key` 必须与当前 issue/workspace metadata 完全匹配；`issue_identifier` 变化按 `identifier_changed` 诊断或受控 rename/alias 处理，不得直接当作不同 issue。
- snapshot 只在 `service_instance_id`、`run_id` 或明确 startup/recovery 输入能解释当前恢复路径时作为 hint；issue `updated_at` 不能单独证明人工 requeue 或服务恢复。
- snapshot 过期、schema 不支持、identity mismatch 时只能作为诊断，不能进入 prompt。
- snapshot 内的 `diagnostic_resume_hint` 只是缓存，不能直接透传为 prompt `resume_reason`；实现必须每次按 fresh tracker state、startup reason 和 workspace metadata 重新推导。
- 如需精确区分 `crash_recovery` 与 `service_restart`，Web 必须注入 `startup_reason` 和 `previous_service_instance_id`；缺失时降级为 `active_resume`/`unknown_resume` 或 `None`，不得为了填充提示而猜测人工 requeue。

### retry due_at 恢复

第一阶段不实现 retry due_at 恢复。原因是持久化 due_at 会让 snapshot 参与 dispatch gate，必须先解决优先级和失效语义。

第二阶段若要恢复 due_at，必须先定义：

- `retry_due_at` 是负向调度状态源，而不是普通诊断字段。
- tracker active label、manual requeue、snapshot due_at 的优先级。
- 如何识别人工从非 active 改回 `Todo`，例如 tracker `updated_at`/label transition token。
- wall-clock 回拨/跳跃时的最大 clamp 和过期处理。

### 重要边界

- snapshot 第一阶段不是调度事实来源。active issue 没有 snapshot 也可以运行。
- snapshot 不能让非 active issue 自动运行。
- snapshot 中的 `last_prompt_retry_attempt_observed` 只用于诊断，不用于重启后的 prompt attempt；`resume_reason` 必须按上面的推导规则生成。
- snapshot 不得覆盖 fresh tracker transition；无法判定时降级为 `active_resume`/`unknown_resume`。

## 方案 C：workspace metadata、健康检查和 fencing

### metadata 文件

每个 issue workspace 内写：

```text
<issue_workspace>/.symphony-workspace.json
```

建议字段：

```json
{
  "schema_version": 1,
  "issue_id": "6",
  "issue_id_path_key": "i-36",
  "workspace_key": "i-36-_6",
  "issue_identifier": "#6",
  "init_started_at": "2026-05-23T08:00:00Z",
  "initialized_at": "2026-05-23T08:00:20Z",
  "init_status": "ready",
  "after_create_fingerprint": "sha256",
  "initialized_by_run_id": "uuid",
  "expected_git_root": ".",
  "last_error": null
}
```

`last_error` 为结构化对象：

```json
{
  "code": "after_create_failed",
  "message": "git clone failed",
  "at": "2026-05-23T08:00:10Z"
}
```

不把 `project_id`、`repo_url` 作为第一阶段必填字段。repo 校验从 workflow/hook 约定派生，避免把 Web 项目模型塞进 rust-platform workspace。若启用 Git 检查，必须通过 `expected_git_root` 明确 repo 在 workspace 根还是子目录。

### 初始化流程

1. 使用 raw `issue_id` 计算 `issue_id_path_key`，并先获取 canonical issue lock。
2. 在 canonical issue lock 下按 `issue_id_path_key` 发现已有 canonical workspace、legacy workspace 和 metadata。
3. 获取 live writer lock。
4. 创建 workspace 目录。
5. 原子写 metadata：`init_status=initializing`。
6. 运行 `after_create`。
7. 执行健康检查。
8. 成功后原子写 `init_status=ready`。
9. 失败则写 `init_status=failed` 和 `last_error`。
10. 继续持有 live writer lock，直到 `after_run` 和最终 snapshot/log update 完成。

ready metadata 是 workspace 初始化的提交记录，必须有单独提交协议：

- metadata 写入必须使用 temp file + fsync file + atomic rename + fsync parent directory；workspace root、`.symphony` 目录和 metadata 父目录必须在关键创建/rename 后 fsync，平台不支持目录 fsync 时必须记录能力降级并用更保守健康检查补偿。
- `init_status=ready` 必须是最后一个持久化动作；`after_create`、hook 产物、最小 Git/目录健康检查和 metadata `initializing`/`failed` 更新全部完成并落盘后，才能写 `ready`。
- 崩溃恢复时不能只相信 `ready` 字段；还必须重新执行 MVP 健康检查，确认 metadata identity、workspace 非空、expected git root 不逃逸、lock fencing 都满足后才可进入 Codex。
- `after_create` 失败后不得在同一个半初始化目录上直接重跑 hook。默认策略是：若目录为空且 metadata 仍匹配，可删除后重建；若目录非空但无 dirty/user changes，可在 canonical lock 下把目录原子隔离到 `.symphony/quarantine/<issue_id_path_key>/<run_id>` 后新建；若存在 dirty/user changes、metadata mismatch 或无法证明安全隔离，则进入 `workspace_init_failed_requires_manual`，释放 claim，不 retry。

### Workspace API 边界

当前按 `identifier` 单参数创建 workspace 的入口必须从 worker run 路径废弃。第一阶段需要引入显式 issue identity API，例如：

```rust
async fn prepare_issue_workspace(
    issue_id: &str,
    issue_identifier: &str,
    run_id: &str,
    service_instance_id: &str,
) -> Result<WorkspaceRunLease, WorkspaceEligibilityError>
```

`WorkspaceRunLease` 至少包含 workspace path、`issue_id_path_key`、`workspace_key`、metadata health 结果、canonical lock guard/live writer guard 以及用于最终诊断写入的 fencing 信息。`AgentRunner` 不得继续直接调用 `ensure_workspace(identifier)` 作为 worker run 入口；orchestrator 必须通过统一 workspace eligibility gate 获得 lease 后才能启动 runner。

### MVP 健康检查

第一阶段只做最小 ready manifest：

- 在创建 `<issue_id_path_key>-<sanitized_identifier>` 新 workspace 前，必须先探测旧 `<sanitized_identifier>` legacy workspace。
- 如果发现多个 `<issue_id_path_key>-*` canonical workspace，必须进入 `workspace_identity_ambiguous_requires_manual`，不得自动任选一个。
- legacy workspace 存在时，不得静默创建新 key workspace 并进入 Codex。
- metadata 存在且 schema 支持。
- metadata `issue_id`、`issue_id_path_key`、`workspace_key` 必须与当前 issue 和 workspace path 完全匹配。
- `issue_identifier` 是可读属性，不是稳定身份。若 metadata `issue_id` 匹配但 `issue_identifier` 与当前 issue 不同，必须进入 `identifier_changed` 诊断或受控 rename/alias 流程；不得创建第二个 workspace，也不得当作普通 `identity_mismatch` 直接丢失旧 workspace。
- `init_status=ready`。
- workspace 目录不为空。
- 如果启用 Git 检查，则 `expected_git_root` 必须是有效 Git 仓库，且不能通过父目录逃逸满足检查。
- metadata 缺失的非空目录第一阶段一律不进入 Codex run。
- `issue_id_path_key` 默认使用 `i-` + lowercase hex(utf8(raw_issue_id))；若生成后的文件名超过实现定义上限，必须使用 `h-` + full SHA-256 lowercase hex，并在 metadata 中保存 raw `issue_id` 且每次健康检查强制校验。阈值必须固定进测试，不能按当前文件系统静默变化。

高级 Git 健康检查放到第二阶段：

- Git 仓库不能是 `No commits yet` 的空仓库。
- remote 与项目 repo 匹配。
- dirty tree 不自动删除。
- hook fingerprint 变化时进入人工诊断或重新初始化策略。

### 半初始化处理

- 空目录：可删除后重新初始化。
- metadata=`initializing` 且超过超时：标记 failed；若目录为空可删除后重新初始化，若目录非空必须按上面的 quarantine 规则隔离后重建，不能原地重跑 `after_create`。
- metadata 缺失但目录非空：`workspace_unknown_requires_manual`，不进入 Codex run；后续可以另设一次性迁移工具。
- legacy workspace 非空：必须在同一 live lock/fencing 语义下进入只读诊断；第一阶段固定为 `workspace_legacy_requires_manual`，要求人工迁移，不进入 Codex。
- 第一阶段发布策略固定为只读诊断并要求人工迁移；不自动迁移、不自动 alias，不运行 hook、不执行 cleanup、不触发任何 workspace 副作用。只读诊断最少输出 legacy path、目标 canonical path、issue_id、issue_identifier、是否存在 lock sidecar、是否检测到 dirty tree。
- metadata raw `issue_id` 或 `issue_id_path_key` mismatch：`workspace_identity_mismatch`，释放 claim，保留诊断，不 retry。
- metadata `issue_id` 匹配但 `issue_identifier` 变化：`identifier_changed`，在 canonical lock 下保留旧 workspace，按受控 rename/alias 或人工诊断处理，不得静默创建新 workspace。
- 多个 `<issue_id_path_key>-*` canonical workspace：`workspace_identity_ambiguous_requires_manual`，释放 claim，保留诊断，不 retry。
- 有未提交更改：不自动删除，失败并进入人工诊断。
- remote 不匹配、repo 空壳、dirty tree：第二阶段 advanced health checks 处理；有用户变更时不得自动删除。

### workspace lock/fencing

必须增加两层锁，防止旧进程和新进程同时操作同一个 issue workspace：

- 进程内 `issue_id -> async Mutex`：防同一个 rust-platform 进程内重复 worker。
- 跨进程主锁必须是 workspace root 下按稳定 issue identity 编码的 canonical issue lock，例如 `<workspace_root>/.symphony/locks/issues/<issue_id_path_key>.lock`。主锁不能使用 raw `<issue_id>`、`<issue_workspace>.lock` 或包含 `sanitized_identifier` 的路径。
- OS lock 文件和持久 sidecar 必须拆分，不能把同一个 `.lock` 路径同时作为 `flock`/`fcntl` 锁 inode 和 temp+rename 更新的 JSON 内容。推荐布局：
  - `<workspace_root>/.symphony/locks/issues/<issue_id_path_key>.lock`：稳定 inode 的 OS lock 文件，只用于互斥；一旦创建，在 workspace root 生命周期内不得 unlink/delete/rename，包括 terminal cleanup、manual cleanup 和错误恢复。
  - `<workspace_root>/.symphony/locks/issues/<issue_id_path_key>.json`：持久 lock sidecar，记录 owner/fencing/child pid 等诊断信息，可 temp+rename 原子更新。
- 必须先获取 canonical issue lock，才能探测 legacy/new workspace、读取 metadata、执行 alias/migration、运行 `after_create` 或进入 Codex。
- terminal cleanup / before_remove 也必须按 `issue_id_path_key` 获取同一个 canonical issue lock 后执行，不得继续使用 identifier-only cleanup 入口删除 workspace。
- `<issue_workspace>.lock` 最多作为 sidecar 诊断文件，不得作为互斥主锁。
- canonical lock sidecar 内容包含 `issue_id`、`run_id`、`service_instance_id`、`service_generation`、`web_instance_id`、Symphony pid/pgid/session/start_time/cmdline/workdir、Codex pid/pgid/session/start_time/cmdline/workdir、workspace path、created_at、updated_seq。
- lock sidecar 是持久诊断/fencing 文件；即使 OS file lock 随 Symphony 死亡释放，新进程也必须读取旧 sidecar 内容并验证 Codex pid/pgid 后才能接管。
- lock sidecar 更新必须使用 temp file + fsync file + atomic rename + fsync parent directory，并带 `updated_seq`/`run_id` fencing，旧 writer 不能覆盖新 writer。
- 旧 writer 更新 sidecar 前必须仍持有稳定 `.lock` inode 上的 OS lock，并校验当前 sidecar 的 `run_id`/`updated_seq` 未被新 owner 接管；否则只能写失败日志，不能 rename 覆盖新 sidecar。验收必须覆盖“rename sidecar 不会释放或替换 OS lock inode”的场景。
- terminal cleanup 可以删除 workspace 目录、清空或 tombstone JSON sidecar，但不得删除 canonical OS lock 文件；若需要清理 locks 目录，只能在全局停机且证明没有任何进程可能持有或打开旧 inode 的离线维护工具中执行，不属于第一阶段运行路径。
- 新进程发现 lock 时先按与 Web PID 验证等价的规则验证 owner 进程组和 Codex child 是否仍活着：PID 存在、cmdline/executable、workdir、start_time、pgid、session、service_instance_id/run_id 可解释时全部匹配；只检查 pid 或进程名不够。PID 复用或身份不匹配时不能当作 live owner，也不能直接删除，必须进入 stale-lock 接管流程或人工诊断。
- owner 活着：不得重建 workspace，不得进入 `before_run`、Codex run 或平台副作用；当前 run 等待或失败。
- owner 已死：必须先确认旧 service session/tree 已清理，再接管 lock，写入 fencing 记录。

lock 生命周期必须覆盖整次 worker run：

```text
prepare_issue_workspace -> before_run -> Codex turn loop -> after_run -> snapshot update
```

只锁初始化不够；否则旧 worker 和新 worker 仍可能同时写代码、更新 workpad、创建 PR/MR。

Codex 子进程启动必须有注册屏障，避免 spawn 后、lock 更新前崩溃导致新进程误接管：

- AgentRunner 启动 Codex/app-server 后，必须先观测 child pid/pgid/session/start_time/cmdline/workdir，把它们写入 canonical lock sidecar 并 fsync，确认 sidecar 已持久化后，才允许 Codex 进入 workspace、执行 `before_run` 之后的副作用或开始 turn loop。
- 如果现有 Codex 启动接口无法在副作用前暴露 child identity，MVP 必须把 Codex 放入可枚举的 service containment，或在启动后立即进入保守 `workspace_locked_by_live_process` 直到能证明 child identity；不得把该窗口留给普通运行。
- Symphony 崩溃但 Codex orphan 仍活着时，新的 orchestrator 必须通过 canonical lock sidecar、workspace path、service_instance/run_id、cmdline/workdir 扫描发现该 child；发现 live child 时不得进入 `before_run`、Codex run、cleanup 或 workspace quarantine。

`resume_reason=workspace_recovery` 只能在 workspace eligibility 已通过、workspace 可进入 Codex 时作为 prompt hint。`locked_by_live_process`、`legacy_requires_manual`、`identity_mismatch`、`unknown_requires_manual` 等非 ready 或 manual-required 状态只能进入日志/API/snapshot 诊断，不能进入 prompt ABI。

terminal cleanup 也必须复用 workspace discovery、metadata 和 dirty/manual-required 检查：

- 0 个 canonical workspace：只清理 snapshot/diagnostic。
- 1 个 canonical workspace：仅当 metadata schema 支持、raw `issue_id`/`issue_id_path_key` 匹配、无 live owner、无 dirty/user changes、无 manual-required 状态时，才可运行 `before_remove` 并删除。
- 多个 `<issue_id_path_key>-*` workspace：进入 `workspace_identity_ambiguous_requires_manual`，不自动选择、不自动批量删除。
- metadata 缺失、identity mismatch、dirty tree、legacy workspace 或 manual-required 状态：保留诊断，不自动删除。
- terminal cleanup 不得调用 identifier-only `remove_workspace`/`cleanup_terminal_workspaces`。

lock lease 所有权必须显式建模，避免 worker task 提前释放：

- 推荐由 orchestrator event loop 在 spawn 前获取 `WorkspaceRunLease` 并存入 `RunningEntry`。
- WorkerExit 后，event loop 必须先写 retry/snapshot/最终诊断，再 drop `WorkspaceRunLease`。
- 如果 lock guard 必须在 worker task 内持有，则 WorkerExit 事件必须把 lease 交回 event loop，事件处理完成后才释放。
- 不允许 `AgentRunner` 在 `after_run` 后立即释放 canonical lock，而 event loop 之后才写 snapshot；这个间隙会允许新进程接管并读到 stale snapshot。

### workspace eligibility 到 retry/claim 的映射

- live writer lock 被活 owner 持有：queue delay，不增加 prompt `attempt`，不记录 failure retry。
- 无并发槽：queue delay，不增加 prompt `attempt`。
- tracker fetch/revalidation unavailable：pre-spawn delay，不增加 prompt `attempt`，不记录 worker failure；retry/continuation entry 保留原 attempt/kind/context。
- metadata 缺失但目录非空、legacy workspace 非空、identity mismatch、dirty tree、manual-required 状态：释放 claim，保留诊断，不 retry。
- `after_create` 或初始化 hook 失败：只有在目录为空可重建，或非空目录已按 quarantine 规则安全隔离并重新创建后，才可按明确上限进入 workspace-init retry；不得把半初始化目录标 ready，也不得原地重跑 hook。超过上限后保留 `workspace_init_failed`/`workspace_init_failed_requires_manual` 诊断并停止 retry。
- `init_status=failed` 且错误需要人工处理：释放 claim，保留诊断，不 retry。
- normal exit 后 re-fetch 失败：不能 schedule continuation；释放 claim，记录诊断，等待下一轮 poll。

queue delay 的实现可以保留 claim 并建立独立 delay entry，也可以释放 claim 等下一轮 poll；但必须保证不会增加 prompt attempt，不会写 failure retry。

来自 retry/continuation entry 的 no-slot、live-lock 和 revalidation-unavailable delay 必须保留原 `retry_attempt`、`RetryKind` 和 `RunContext`。不得释放后只靠普通 poll 重新发现而丢失 attempt；“释放 claim 等下一轮 poll”只允许用于首次候选，或必须同时写入等价 pending-attempt 记录。

## Web 平台进程生命周期补强

Web 平台不保存 issue 级恢复摘要，但必须保证项目级进程生命周期不会产生重复执行。

### service tree 管理

- 启动 Symphony 时继续使用新 process group。
- stop/restart/cleanup 必须区分 graceful root stop 与 hard tree cleanup。
- graceful root stop：先只通知 rust-platform 根进程或控制面，让它取消 worker、运行 `after_run`、写诊断 snapshot、清理 Codex。
- hard tree cleanup：grace period 后再枚举同 session 或 descendant process groups，对残留 Symphony/Codex 进程发 SIGKILL。
- 停止后验证 Symphony、Codex app-server 和 descendants 都退出。
- hard cleanup 不能只依赖父子进程树。Web/startup cleanup 必须同时扫描项目 workspace root 下的 canonical issue lock sidecars 和 resume diagnostics，按 `service_instance_id`、`run_id`、workspace path、cmdline/workdir/start_time 反查 Codex pid/pgid/session。即使 Codex app-server 自己 `setsid`、父 Symphony 被 `kill -9` 后变成 orphan，仍必须能发现并清理，或至少让新 worker 因 live lock 进入 `workspace_locked_by_live_process` 且 API 可查询残留 pid/pgid。
- 如果目标平台支持更强 containment（例如 job object、cgroup、launchd group），可以作为 Web cleanup 的实现细节；但行为契约仍是按 service_instance/workspace/lock 可发现所有 Symphony/Codex descendants 和 orphan。

需要新增或封装：

- `kill_process_group(pgid)`。
- `graceful_stop_root(pid, service_instance_id)`。
- `terminate_service_tree(service_instance_id, pid, pgid, session_id)`。
- `verify_no_service_descendants(service_instance_id, workdir, session_id)`。
- `discover_service_orphans(service_instance_id, workspace_root, cmdline_fingerprint)`：扫描 DB reservation、argv fingerprint、canonical issue lock sidecars、workspace lock sidecars 和 OS process table。
- `cleanup_orphan_codex_from_locks(service_instance_id, workspace_root)`：清理或报告已脱离父子树但仍匹配 workspace/service identity 的 Codex child。

### PID 验证

PID 验证需要包含：

- PID 存在。
- executable path 或 command/cmdline 是预期 Symphony。
- workdir 匹配。
- 启动时间或 OS process start time 匹配，防止 PID 复用。
- process group id 和 session id 匹配。

macOS/Linux 实现细节可以不同，但行为必须等价；不能只用进程名包含 `symphony` 判断。

### DB lifecycle fencing schema

Web service state 需要增加可 CAS 的生命周期字段：

- `web_instance_id`：当前 web-platform 进程实例 UUID。
- `lifecycle_op_id`：一次 start/stop/restart/cleanup 操作 UUID。
- `lifecycle_lease_expires_at`：生命周期操作 lease 过期时间，需 heartbeat 或短 TTL。
- `service_owner_web_instance_id`：当前 running/starting service 的长期 owner Web 实例。
- `service_owner_lease_expires_at`：service owner heartbeat lease 过期时间，不等同于 lifecycle operation lease。
- `service_owner_heartbeat_at`：owner Web 最近一次 heartbeat 时间。
- `service_generation`：每次 start/restart 递增。
- `service_instance_id`：一次子进程实例的 UUID，并通过 env/argv 注入 rust-platform。
- `service_pid`、`service_pgid`、`service_session_id`。
- `service_started_at` 或 OS process start time。
- `service_cmdline_hash`、`service_workdir`。
- `last_lifecycle_op`。
- 持久化 `restart_count`。

所有 start/stop/restart/watcher 状态更新必须带 generation/instance CAS，例如 `WHERE service_generation = ? AND service_instance_id = ?`。如果 CAS 失败，当前操作必须停止并清理自己刚启动的进程。

所有会产生外部副作用的 lifecycle 操作（spawn、graceful stop、hard cleanup、startup cleanup）必须先原子获得 lifecycle lease，再执行副作用。lifecycle lease 只覆盖一次操作，不能表达 running service 的长期归属；长期归属必须使用 `service_owner_*` lease。

Web 启动 rust-platform 时必须先预生成并注入 `service_instance_id`、`service_generation`、`web_instance_id`。OS pid/pgid/session 在 spawn 后由 Web 观测并 CAS 写入 DB；rust-platform 启动后自检 `pid/pgid/session` 并写入 workspace lock/log。不得要求 Web 在 spawn 前注入尚不可知的 pgid/session。

### Web -> rust-platform launch ABI

Web 管理启动 rust-platform 时必须使用固定 env ABI，不能让 Web 和 Rust 各自生成互不相关的 instance id：

| Env | 含义 | Web 管理启动 | standalone CLI |
|---|---|---|---|
| `SYMPHONY_WEB_INSTANCE_ID` | 当前 Web 平台进程实例 UUID | 必填 | 可缺省，rust-platform 生成本地值 |
| `SYMPHONY_SERVICE_INSTANCE_ID` | 当前项目服务实例 UUID | 必填 | rust-platform 生成本地值 |
| `SYMPHONY_SERVICE_GENERATION` | DB 中的 service generation | 必填 | `0` 或缺省 |
| `SYMPHONY_STARTUP_REASON` | `api_start`、`api_restart`、`watcher_restart`、`startup_cleanup_restart`、`standalone` | 必填 | `standalone` |
| `SYMPHONY_PREVIOUS_SERVICE_INSTANCE_ID` | watcher/crash restart 前一个实例 id | crash/service restart 时必填；其他可空 | 空 |
| `SYMPHONY_LIFECYCLE_OP_ID` | 本次 Web lifecycle 操作 UUID | 必填 | 空 |

规则：

- Web 管理启动缺少必填 env 时，rust-platform 必须 fail fast，不能静默降级成 standalone。
- Web 管理启动必须同时把 `service_instance_id`、`service_generation`、`lifecycle_op_id` 作为可被 OS 进程枚举稳定识别的 argv flag 注入；env 是运行时 ABI，不得作为唯一 orphan discovery 依据。
- standalone CLI 不参与 Web DB fencing，但仍必须生成 `service_instance_id`/`run_id` 写入 workspace lock、snapshot 和日志。
- rust-platform 启动后必须自检自身 pid/pgid/session 与 Web 注入的 generation/instance 语义一致；自检失败时退出，不进入 poll/worker。
- `startup_reason` 是 `resume_reason` 推导输入，不得直接无条件透传为 prompt `resume_reason`。

### start/restart/watcher reservation 状态机

所有会 spawn 新 rust-platform 的路径（API start、API restart、watcher auto-restart、startup cleanup restart）必须采用同一个 DB reservation 算法：

1. 获取进程内 per-project lock。
2. 用 `BEGIN IMMEDIATE` 或单条条件 `UPDATE` 原子获取 lifecycle lease；同时写入新的 `lifecycle_op_id`、预生成的 `service_instance_id`、递增后的 `service_generation`、`web_instance_id`、`status=starting`、`lifecycle_lease_expires_at`。
3. 只有 affected rows = 1 或事务提交成功才允许 spawn；否则当前操作退出，不产生外部副作用。
4. spawn 时注入上节固定 env ABI。
5. spawn 成功后由 Web 观测 pid/pgid/session/start_time/cmdline hash/workdir，并用 `WHERE lifecycle_op_id=? AND service_instance_id=? AND service_generation=?` CAS 写入 `running`。
6. CAS 写入 `running` 时，必须同时写入 `service_owner_web_instance_id=current_web_instance_id`、刷新 `service_owner_lease_expires_at` 和 `service_owner_heartbeat_at`；ProcessManager/watcher 存活期间必须 heartbeat 该 owner lease。
7. 如果 CAS 失败、ProcessManager 注册失败、child wait task 创建失败或 watcher task 创建失败，必须按刚 spawn 的 pid/pgid/session 立即 terminate 自己启动的 service tree。
8. spawn 失败时用同一 lifecycle identity CAS 写入 failed/stopped，并释放 lease；CAS 失败只记录告警，不能覆盖新 instance。
9. 任何旧 generation/旧 instance 的 wait/watcher 回调只能 CAS 写回旧 instance；CAS 失败即退出。
10. reservation 条件必须排除“其他 Web instance 的 `service_owner_lease_expires_at` 未过期”的 running/starting service；这种状态下 API start/restart/watcher/startup cleanup 都不得 spawn 或接管。

### spawn-after-reservation crash recovery

必须覆盖 Web 在 spawn 成功后、pid/pgid/session CAS 写入 DB 前崩溃的窗口。

规则：

- reservation 阶段必须写入足以发现启动意图的持久信息：`service_instance_id`、`service_generation`、`lifecycle_op_id`、`service_workdir`、expected cmdline/argv fingerprint、reservation `created_at`。
- Web 启动 rust-platform 时必须把 `service_instance_id`、`service_generation`、`lifecycle_op_id` 以 argv flag 注入，使 startup cleanup 可以通过 OS 进程枚举稳定识别；env 仍保留给 rust-platform 运行时读取，但不能作为唯一发现依据。
- startup cleanup 遇到 `starting` 且缺少 pid/pgid/session 的记录时，不得直接标 stopped 或重新 start；必须按 `service_instance_id` + `service_workdir` + cmdline/argv fingerprint 扫描 OS 进程。
- 若发现匹配进程，必须先补写 pid/pgid/session 后执行 owner-gated cleanup，或直接 terminate 对应 service tree 并 verify no descendants，再用 generation/instance/lifecycle CAS 写 stopped。
- 若未发现匹配进程，且 lifecycle lease 已过期，才允许 CAS 写 stopped/failed。
- `spawn 成功但 DB/ProcessManager 更新失败` 的验收必须包含 Web crash-before-pid-CAS 场景。

### API stop/restart owner gate

API stop 和 API restart 的 stop phase 也必须受 service owner lease 约束，不能只受 lifecycle operation lease 约束。

规则：

- 如果 `service_owner_web_instance_id != current_web_instance_id` 且 `service_owner_lease_expires_at` 未过期，当前 Web 不得执行 `graceful_stop_root`、`terminate_service_tree`、状态覆盖或 restart spawn。
- 非 owner Web 收到 stop/restart 请求时，必须返回 conflict/accepted-for-owner-delegation，或通过后续明确设计的跨 Web owner RPC 转发；第一阶段不实现跨 Web RPC 时固定返回 conflict/accepted，不做副作用。
- 只有 owner 是当前 Web，或 owner lease 已过期，才允许获取 lifecycle stop/restart lease。
- 获取 stop/restart lifecycle lease 后必须重新读取 DB generation/instance/owner lease，并用 `WHERE service_generation=? AND service_instance_id=? AND service_owner_web_instance_id=?` CAS 写入 stopping/stopped/restarting。
- restart 必须先按上述 owner gate 成功完成旧 service tree cleanup 并验证 descendants 退出，再进入统一 start reservation；不得在非 owner 有效时先 kill 再尝试 reservation。
- stop/restart 清理失败时不得清除 owner lease，不得写 stopped/running 新实例，只记录诊断。

ProcessManager 必须持有或管理：

- `service_generation`、`service_instance_id`、`web_instance_id`、`lifecycle_op_id`。
- `service_owner_web_instance_id`、owner heartbeat task/interval/deadline。
- child handle 或专门 wait task，用于 reap 子进程并获得真实退出结果。
- watcher task handle 和 cancel token。
- 与 generation/instance 绑定的 in-memory state。
- startup cleanup 和 SIGTERM cleanup 所需的 service tree 信息。
- 旧 generation 的 wait/watcher 回调只能 CAS 写旧 instance；CAS 失败不得覆盖新状态。

PID 验证是辅助信号；运行状态主判定应优先使用 child wait result、DB generation/instance 和 lifecycle lease。

### per-project lock

所有项目级生命周期操作必须共用同一个 per-project lifecycle lock。进程内 lock 只能防同一 Web 实例内的竞态；跨 Web 实例必须依赖 DB lifecycle fencing/CAS 和 lifecycle lease：

- API start。
- API stop。
- API restart。
- watcher auto-restart。
- startup cleanup。
- Web 平台 SIGTERM 全局 shutdown。

watcher 不得绕过锁直接 spawn。

watcher 在 crash backoff 后必须重新获取 lock，并重新读取 DB 与 ProcessManager generation：

- 比较 backoff 前捕获的 `service_generation`/`service_instance_id`。
- 确认 lifecycle lease 仍由当前 `web_instance_id` 持有，或旧 lifecycle owner lease 已过期。
- 确认当前 Web 是 `service_owner_web_instance_id`，且 owner lease heartbeat 未过期；非 owner watcher 不得运行。
- 确认 DB 状态仍允许 auto-restart，例如 `error` 且 restart_count 未超限。
- 如果用户已手动 stop/start，旧 watcher 不得 spawn。
- 如果 service instance 已变化，旧 watcher 退出。
- 如果 project 不再处于可自动恢复状态，旧 watcher 退出。

spawn 成功但 DB 或 ProcessManager 更新失败时，必须立即 terminate 自己刚启动的 service tree，不能留下 orphan Symphony。

watcher backoff sleep 必须可取消；stop/restart/SIGTERM 取消 watcher 后，旧 watcher 不得睡醒再竞争 lifecycle lock。`ProcessManager` 必须提供按 instance cancel/join watcher 的 API；stop/restart/SIGTERM 在 graceful/hard cleanup 前必须先取消并 join 对应 watcher。

### Web 平台 SIGTERM

Web 平台收到 SIGTERM 时：

1. 停止接受新请求。
2. shutdown 入口必须持有 `AppState`/repo/process_manager，不能只返回 Axum graceful shutdown future。
3. 停止或 join watcher tasks，防止 cleanup 期间 watcher 再 spawn。
4. 只枚举当前 Web 实例拥有或 owner lease 已过期的 running/starting/error project。若 `service_owner_web_instance_id != current_web_instance_id` 且 `service_owner_lease_expires_at` 未过期，必须跳过该 project，不得 cancel 其他 owner 的 watcher、不得 graceful stop、不得 hard cleanup、不得 CAS stopped、不得清 owner lease。
5. 对通过 owner gate 的 project 获取生命周期锁，并在锁内重新读取 generation/instance/owner lease。
6. 对每个仍由当前 Web 拥有或 owner lease 已过期且已接管的 project，先调用 `graceful_stop_root`。
7. grace period 后对未退出 descendants 调用 hard `terminate_service_tree`。
8. 更新项目 service state；CAS 失败时记录告警。
9. 全局 shutdown 必须有最大等待时间；超时后记录 `service_tree_cleanup_failed` 并按配置决定是否继续退出。
10. 不直接写 issue 级恢复摘要；issue 级 snapshot 由 rust-platform graceful shutdown 写入。若 rust-platform 未能写入，重启后仍靠 active label + workspace metadata 恢复。

需要新增明确实现入口：

- `ProcessManager::shutdown_all(repo, deadline)`：持有 `AppState`/repo/process_manager，枚举 running/starting/error project；对其他 Web instance 且 owner lease 未过期的 service 只记录 skip，不执行任何副作用；对当前 Web owner 或 owner lease 过期并成功接管的 service，cancel/join 当前 instance watcher，graceful root stop，hard tree cleanup，verify descendants，最后用 generation/instance CAS 更新状态。
- `startup_cleanup_with_lifecycle_fencing(repo, process_manager, web_instance_id)`：启动时先读取 running/starting/error project；如果 `service_owner_web_instance_id` 属于其他 Web instance 且 `service_owner_lease_expires_at` 未过期，必须跳过，不得 cleanup、不得 restart、不得接管 watcher。只有 owner lease 过期，或 owner 是当前 `web_instance_id`，才允许获取 lifecycle cleanup lease 并执行 service tree cleanup；cleanup 前必须重新读取 DB generation/instance 并验证 pid/pgid/session/cmdline/workdir。

Web SIGTERM 在 owner gate 通过、cancel watcher、graceful/hard cleanup 完成并验证 descendants 后，才能 CAS 清除当前 service owner lease 或写入 stopped。cleanup 失败时不得假装 owner 已释放；必须保留诊断，避免其他 Web 实例误接管未清理的 live service tree。

## Label 写入边界

硬结论：

- Orchestrator 只读 tracker label。
- Agent/人工负责 workflow label 变更。
- 第一阶段删除或禁用 orchestrator terminal `set_workflow_state` 路径，并通过只读 tracker trait/wrapper 让 orchestrator 在类型边界上无法写 workflow label。

如果未来要让 orchestrator 写 label，必须另起设计，改成“双写 + CAS/lock”模型；不能在本方案中作为可选分支混入。

## 状态流转语义

### 正常执行

1. poll fetch active issue。
2. scheduler 判断未 claimed、有 slot、无 active blocker。
3. workspace health ok。
4. spawn worker。
5. agent 看到 `Todo` 时改为 `In Progress`。
6. agent 查找或创建唯一 workpad。
7. 完成后 agent 移到 `Human Review`。
8. worker normal exit 后 orchestrator 重新 fetch tracker state。
9. fresh state 非 active 时释放 claim，不写 continuation retry。

### 同进程 failure retry

1. worker abnormal exit。
2. 同进程内写入 failure retry 队列：`retry_kind=Failure`、`retry_attempt=N`、`backoff_attempt=N`。
3. 写诊断 snapshot：`last_lifecycle=worker_failed`、`last_prompt_retry_attempt_observed=N`、`last_error`。
4. retry timer fired。
5. 重新 fetch issue 当前状态。
6. 只有当前状态仍在 `active_states` 才 spawn。
7. prompt 收到 `attempt=N`，并且 main template 或 retry/resume template 实际使用该值。

### 服务重启恢复

1. rust-platform graceful shutdown 尝试写诊断 snapshot：running issue 标记 `stopping`。
2. Web 平台停止 service session/tree。
3. 重启后 rust-platform 读取 snapshot。
4. poll fetch active issue。
5. 按 `resume_reason 推导规则` 生成保守 hint；只有可证明的正常重启才使用 `service_restart`，否则使用 `active_resume`/`unknown_resume`。
6. agent 复用 workpad 并 reconcile。

### 崩溃恢复

1. Web watcher 发现 process group 不健康。
2. 获取 lifecycle lock，并用 DB generation/instance CAS 确认当前 service 仍是 crash 前捕获的实例。
3. terminate 旧 service session/tree，验证退出。
4. 按 auto-restart 策略最多启动一个新实例，并写入新的 generation/instance。
5. 新 rust-platform 读取 snapshot；如果没有 snapshot，也按 active label + workspace metadata 恢复。
6. prompt 获得按规则推导出的 `crash_recovery`、`workspace_recovery`、`active_resume` 或 `unknown_resume`。

### 人工改回 Todo

语义固定为：进入 active 调度候选，保留 workpad/workspace 作为上下文。第一阶段没有持久 `retry_due_at`，因此不需要判断并清除旧 due_at。

流程：

1. 人工把 `In Progress`、`Human Review` 或其他状态改为 `Todo`。
2. 下一轮 poll 发现 active。
3. 第一阶段不精确识别 manual requeue：如果存在匹配且未过期的 snapshot/workspace metadata，可使用 `resume_reason=active_resume`；否则使用 `resume_reason=None` 或 `unknown_resume`。
4. agent 先把 `Todo` 改为 `In Progress`。
5. agent 查找已有 workpad 并 reconcile，不创建第二个 workpad。

## 非 active 但存在 snapshot

如果 snapshot 显示最近生命周期为 running/stopping/worker_failed，但 tracker 当前状态非 active 且非 terminal：

- 不自动运行。
- 不自动清除 snapshot。
- 标记或保留为 `stopped_unresolved`。
- Web/UI 应显示诊断：需要人工确认是重新入队、清理 workspace，还是保持等待。

这避免“agent 已移到 Human Review 但崩溃在 PR/workpad 更新前”这类状态被静默清理。

## 幂等性要求

- 同一 issue 只能有一个 active worker。
- 同一 issue workspace 只能有一个 live writer，且 lock 覆盖整次 worker run。
- 架构层保证重复 poll、重复 retry fired、服务重启、watcher 重启不能创建第二个 worker，也不能重复进入同一 workspace 的 `before_run`/Codex。
- 同一 issue 只能有一个 `## Codex Workpad`、同一 branch/PR/MR 不重复创建，属于 agent workflow/prompt 和平台查询层能力，但必须纳入第一发布单元的端到端门禁。恢复架构不解析 workpad、不保存 issue 级状态；因此 agent 启动 prompt、平台查询 API 和 tracker/SCM 查询必须形成硬契约：每次 run 开始先按 issue id 查找唯一 workpad、现有 branch、现有 PR/MR 和上次 run marker，再决定续接或创建。
- 若发现多个 workpad、多个候选 branch/PR/MR，或无法确认外部副作用是否已发生，agent 不得创建新的外部副作用；必须进入 `external_effects_ambiguous_requires_manual` 诊断。服务崩溃后 active issue 重新执行时，这条契约是防止重复 PR/MR/workpad 的发布门禁之一。

## 告警和诊断

需要明确暴露以下状态：

- `workspace_init_failed`
- `workspace_locked_by_live_process`
- `stopped_unresolved`
- `service_tree_cleanup_failed`
- `duplicate_process_prevented`
- `non_active_snapshot_retained`
- `workspace_unknown_requires_manual`
- `workspace_legacy_requires_manual`
- `workspace_identity_mismatch`
- `workspace_init_failed_requires_manual`
- `external_effects_ambiguous_requires_manual`

这些状态不一定都要第一阶段进入 Web UI，但必须进入日志和可查询状态。第一阶段最小可查询契约固定为 per-issue diagnostic JSON + service diagnostic JSON，两者都使用 temp file + fsync + rename 原子写，并可由 API 直接读取或聚合：

```text
<workspace_root>/.symphony/diagnostics/issues/<issue_id_path_key>.json
<workspace_root>/.symphony/diagnostics/services/<service_instance_id>.json
```

issue diagnostic 最少字段：

```json
{
  "schema_version": 1,
  "project_id": "optional-web-project-id",
  "issue_id": "6",
  "issue_id_path_key": "i-36",
  "issue_identifier": "#6",
  "workspace_key": "i-36-_6",
  "run_id": "uuid",
  "service_instance_id": "uuid",
  "status_code": "workspace_locked_by_live_process",
  "severity": "error",
  "message": "human-readable summary",
  "last_observed_tracker_state": "Todo",
  "workspace_path": "/abs/path",
  "owner": {
    "symphony_pid": 123,
    "symphony_pgid": 123,
    "codex_pid": 456,
    "codex_pgid": 456,
    "owner_verified": true
  },
  "updated_at": "2026-05-23T08:00:00Z",
  "next_action": "wait_or_manual_cleanup"
}
```

service diagnostic 最少字段：`project_id`、`service_instance_id`、`service_generation`、`web_instance_id`、`lifecycle_op_id`、`status_code`、`pid/pgid/session/start_time`、`cmdline_hash`、`workdir`、`restart_count`、`updated_at`、`next_action`。查询必须支持按 project、issue_id、run_id、service_instance_id 过滤；日志只作为辅助，不能是唯一诊断入口。

## 实施顺序

### 第一发布单元门禁

在 `retry correctness`、`workspace minimal metadata + live writer lock`、`Web process uniqueness` 三块全部完成并通过验收前，不得开启 auto-restart/resume，也不得宣称第一阶段已解决重复执行风险。尤其不能在没有 workspace live writer lock 的情况下增强 retry/restart 行为。

现有 legacy auto-restart/watcher 在 `Web process uniqueness` 通过前也必须通过 feature flag 关闭，或保持不可发布状态；门禁约束不只针对新增 resume 能力。

1. **retry correctness**
   - 贯通 `retry_attempt`。
   - 确保 retry worker 第一 turn 的 prompt 实际使用 `attempt`/resume 信息。
   - 所有 spawn 前 revalidation 增加 active-state check。
   - tracker fetch/revalidation unavailable 不增加 prompt attempt。
   - 删除、私有化或同 gate 化 integration-layer dispatch/register 绕过入口。
   - normal worker exit 后由 event loop re-fetch fresh tracker state，再决定是否 continuation retry；WorkerExit 携带的状态只能作为诊断，不能作为调度依据。
   - `NoSlot`/queue-delay 不污染 failure retry 和 prompt attempt。
   - 拆分或包装 tracker trait，使 Orchestrator 在类型边界上只读，不能调用 workflow label mutation。

2. **workspace minimal metadata + live writer lock**
   - workspace key 改为 `<issue_id_path_key>-<sanitized_identifier>`。
   - canonical issue lock 改为 `<workspace_root>/.symphony/locks/issues/<issue_id_path_key>.lock`，并在 legacy/new/alias 探测前获取。
   - canonical issue lock 的 OS lock inode 与持久 JSON sidecar 拆分，禁止对持锁 `.lock` 路径做 temp+rename 替换；运行时 cleanup 不得删除 canonical OS lock 文件。
   - 增加 legacy workspace 探测、人工诊断/只读迁移/alias 策略。
   - 增加 workspace metadata。
   - 废弃 worker run 路径上的 `ensure_workspace(identifier)`，改为返回 `WorkspaceRunLease` 的 issue identity API。
   - 增加最小 ready manifest 和 identity mismatch 检查。
   - 增加 ready manifest fsync/rename 提交协议，并在崩溃恢复时重新执行健康检查。
   - 增加覆盖整次 worker run 的 workspace lock/fencing。
   - 增加 Codex child 注册屏障，child pid/pgid/session/start_time/cmdline/workdir 持久写入 canonical lock sidecar 后才允许进入 workspace 副作用。
   - 明确 lease 由 event loop 持有至 snapshot/最终诊断写入完成，或通过 WorkerExit 交回 event loop 后释放。
   - 定义 workspace eligibility 到 retry/claim 的映射。
   - 定义 `after_create` 失败后的删除/隔离/人工诊断策略，不允许半初始化目录原地重跑 hook。
   - 坏 workspace 不启动 Codex。

3. **Web process uniqueness**
   - 增加 DB lifecycle generation/instance/pgid/session schema 和 CAS 更新。
   - 增加 web_instance/lifecycle lease。
   - 固定 Web -> rust-platform launch env ABI，并让 rust-platform fail fast / standalone 明确分流。
   - 所有 start/restart/watcher 先 DB reservation，再 spawn，再 CAS 写 pid/pgid/session；失败补偿清理刚启动的 service tree。
   - ProcessManager 管理 generation/instance、child wait task、watcher handle/cancel token。
   - stop/restart/cleanup 先 graceful root stop，再 hard service tree cleanup。
   - hard cleanup 扫描 canonical issue lock sidecars 和 OS process table，发现脱离父子树的 Codex orphan。
   - watcher 使用 lifecycle lock，并在 backoff 后重新验证 generation/instance。
   - stop/restart/SIGTERM 先 cancel+join 对应 instance watcher。
   - Web SIGTERM 只关闭当前 Web owner 或 owner lease 已过期并成功接管的 project service trees；其他 Web owner lease 有效时必须跳过。
   - PID 验证加入 exact cmdline/workdir/start_time/process group/session。

4. **外部副作用幂等门禁**
   - agent/platform 查询契约必须能按 issue id 查找唯一 workpad、branch、PR/MR 和上次 run marker。
   - 恢复 run 必须先 reconcile 已有外部副作用，不能盲目创建第二个 workpad、branch 或 PR/MR。
   - 多个候选或无法确认时进入 `external_effects_ambiguous_requires_manual`，不继续创建外部副作用。

5. **rust-platform resume JSON hint**
   - 写 per-issue 本地 snapshot。
   - 仅保存 last_error、last_lifecycle、last_run_id、resume hint。
   - 重启读取 snapshot 作为 diagnostic hint / 诊断，不作为 dispatch gate。
   - prompt 支持 `resume_reason`。

6. **UI/API 诊断增强**
   - 显示 stopped_unresolved、workspace init failed、process cleanup failed。
   - 可选消费 rust-platform state/snapshot 作为诊断缓存。

## 验收标准

### retry attempt

- failure retry prompt 包含 `attempt=1`，且 retry worker 第一 turn 实际使用该信息。
- continuation retry prompt 包含 `attempt=1`，且必须基于 fresh tracker state 仍 active。
- first-turn render test 证明 `resume_reason=service_restart` 时 main template 或 retry/resume template 能看到顶层 `resume_reason`。
- no-slot reschedule 不增加 prompt retry attempt，且不记录为 failure retry。
- retry fired 遇 no-slot 后，后续运行仍使用原 `attempt`/`RetryKind`/`RunContext`，不变成 `None` 或 `attempt+1`。
- retry fired 遇 tracker fetch/revalidation unavailable 后不 spawn、不增加 prompt `attempt`，后续仍保留原 `attempt`/`RetryKind`/`RunContext`。
- issue 已移到 `Human Review` 后 retry fired 不再 spawn worker。
- 普通 poll candidate 在 fetch 后被改到 `Human Review`，spawn 前 revalidation 不再 spawn worker。
- integration-layer dispatch/register 入口不能绕过 `spawn_worker(issue, retry_attempt, run_context)` gate。
- worker normal exit 后若 issue 已进入 `Human Review`，不写 continuation 诊断状态，不 schedule continuation retry。
- WorkerExitNormal 携带 stale `still_active=true` 或最后观察状态为 active 时，orchestrator event loop 仍必须忽略该调度判断并重新 refresh；refresh 后非 active 则不写 continuation retry。
- retry/continuation entry 连续遇 no-slot、live-lock、revalidation unavailable 后，最终 spawn 仍保留原 `retry_attempt`/`RetryKind`/`RunContext`；revalidation delay 不污染 prompt attempt，也不消耗 failure retry 次数。
- orchestrator 注入的 tracker 类型不包含 workflow label mutation；`rust-platform/src/orchestrator/**` 不引用 `set_workflow_state`。

### resume snapshot

- 第一阶段 snapshot 不包含 dispatch gate 字段；重启后 active issue 不因 snapshot 被延迟或放行。
- snapshot 写入由 orchestrator event loop 串行完成；worker 不直接写文件。
- 多 issue 并发 worker start/exit/retry/shutdown 后，per-issue snapshot 不丢记录；全局聚合 snapshot 只能串行生成。
- prompt `attempt` 不把 service restart 伪装成同进程 retry；service restart 只通过 `resume_reason` 表达。
- stale snapshot、identity mismatch snapshot 不进入 prompt，只保留诊断。
- 人工改回 active 的场景不强制生成 `active_resume`；只有存在匹配且未过期的 snapshot/workspace metadata 或明确 recovery 信号时才可使用 `active_resume`，否则 `None`/`unknown_resume`。

### workspace 半初始化

- 空目录不会被静默复用。
- 没有 ready metadata 的非空目录不会进入 Codex run。
- 已有 `<sanitized_identifier>` legacy 旧目录存在时，新版不会静默创建 `<issue_id_path_key>-<sanitized_identifier>` 并运行；必须进入人工诊断、只读迁移或显式 alias。
- raw issue id 含 path 特殊字符或仅编码大小写不同的碰撞风险时，workspace、lock、snapshot 文件名全部使用同一个大小写折叠安全的 `issue_id_path_key`；验收覆盖大小写不敏感文件系统碰撞场景。
- issue identifier 变化时不会创建第二个 workspace；`issue_id` 匹配但 identifier 不匹配进入 `identifier_changed` 或受控 rename/alias。
- 多个 `<issue_id_path_key>-*` workspace 同时存在时进入 `workspace_identity_ambiguous_requires_manual`，不自动选择。
- terminal cleanup 使用 issue-id keyed canonical lock，不使用 identifier-only 删除路径。
- terminal cleanup 在 metadata 缺失、identity mismatch、dirty tree、legacy workspace、manual-required 或多个 canonical workspace 时不自动删除。
- legacy workspace 第一阶段固定为只读诊断并要求人工迁移；验收必须包含最小诊断输出字段。
- metadata=`initializing` 且超时会被标记 failed。
- `after_create` 写入部分文件后失败，后续 retry 必须删除空目录或 quarantine 非空安全目录后重建；不得在半初始化目录上原地重跑 hook，不得把半初始化目录标 ready。
- `ready` manifest 写入使用 temp file + fsync file + rename + fsync parent；崩溃恢复时即使看到 `ready` 也会重新执行 MVP 健康检查。
- live writer lock 覆盖 `before_run`、Codex、`after_run` 和最终诊断写入。
- canonical issue OS lock 文件与持久 JSON sidecar 是不同路径；sidecar temp+rename 更新不会替换持锁 inode，旧 writer 不能在失去 fencing 后覆盖新 sidecar。
- terminal cleanup、manual cleanup 和错误恢复不会 unlink/delete/rename canonical OS lock 文件；cleanup 后后续并发 open/lock 仍竞争同一个稳定 inode。
- WorkerExit 后 snapshot/最终诊断写入完成前，canonical lock 不释放。
- Symphony spawn Codex 后、lock sidecar 写入 Codex pid/pgid/session/start_time/cmdline/workdir 前崩溃时，新实例不得误接管；实现必须通过注册屏障或 containment 证明该窗口不可进入 workspace 副作用。
- lock sidecar 中 owner pid 被 OS 复用为无关进程时，workspace lock 验证必须因 cmdline/workdir/start_time/session/service_instance/run_id 不匹配而按 stale 或人工诊断处理。
- legacy/new workspace 探测、metadata 读取、alias/migration 都发生在 canonical issue lock 持有期间。
- workspace key collision 或 metadata issue mismatch 时不进入 Codex，不进入 failure retry。
- live lock 被活 owner 持有时不增加 prompt attempt。
- 有未提交更改的目录不会被自动删除。
- remote 不匹配、dirty tree、空 git repo 属于第二阶段 advanced health checks，有明确人工诊断或隔离策略。

### 进程生命周期

- Web 收到 SIGTERM 后，其当前拥有或 owner lease 已过期并成功接管的 service 无残留 Symphony/Codex 子进程；其他 Web owner lease 有效的 service 必须跳过，不属于当前 Web shutdown 清理范围。
- Web A 收到 SIGTERM 时，如果某 project 的 `service_owner_web_instance_id` 是 Web B 且 owner lease 未过期，Web A 不得 cancel Web B watcher、不得 graceful/hard cleanup、不得 CAS stopped、不得清 owner lease。
- Codex app-server 自己 `setsid` 或父 Symphony 被 `kill -9` 后成为 orphan 时，Web startup cleanup 必须通过 canonical issue lock sidecars/argv fingerprint/workspace path 发现并清理；如果无法清理，新 worker 必须阻塞并暴露 `workspace_locked_by_live_process`。
- `kill -9` Symphony 后 watcher 最多启动一个新实例。
- 并发 start/stop/restart 与 watcher auto-restart 后，每个 project 最多一个 live process group。
- PID 复用或同名 `symphony` 进程在 cmdline/workdir/start_time/process group/session 不匹配时不会误判为原服务。
- spawn 成功但 DB/ProcessManager 更新失败时，新启动的 service tree 会被补偿清理。
- 两个 Web 实例短暂共存时，DB lifecycle CAS 防止同一 project 双启动。
- API start/restart/watcher 都先成功 DB reservation 才允许 spawn；reservation CAS 失败不会产生进程。
- rust-platform 能读取并校验固定 lifecycle env；Web 管理启动缺失 env 时 fail fast，standalone CLI 明确标记 `startup_reason=standalone`。
- Web crash-before-pid-CAS 场景下，startup cleanup 能通过 persisted reservation + argv fingerprint 找到并清理或接管刚 spawn 的 orphan rust-platform；找不到且 lease 过期时才写 stopped/failed。
- API stop/restart 如果命中其他 Web instance 且 owner lease 未过期，不执行 graceful/hard cleanup、不覆盖状态、不启动新实例。
- restart 必须先通过 owner gate 完成旧 service tree cleanup 并验证 descendants 退出，再进入统一 start reservation。
- stop/restart/SIGTERM 会 cancel+join 对应 watcher，旧 watcher backoff 醒来不会启动新实例。
- startup cleanup 不会清理 lifecycle lease 仍有效的其他 Web 实例服务。
- graceful stop 先给 rust-platform 根进程退出机会，超时后才 hard kill descendants。
- 诊断 API/JSON 可按 project、issue_id、run_id、service_instance_id 查询 `workspace_locked_by_live_process`、`service_tree_cleanup_failed`、`stopped_unresolved` 等状态；分散日志不能作为唯一验收证据。

### 恢复和幂等

- 服务停止后人工把 issue 改回 `Todo`，重启后 agent 复用已有 workpad，不创建第二个 workpad。
- agent 已创建 branch/PR/MR/workpad 但未改 label 时服务崩溃，重启后的 active run 必须通过平台查询/workpad reconcile 续接，不创建第二个外部副作用；多个候选时进入 `external_effects_ambiguous_requires_manual`。
- 旧进程未退出时，新进程不能重建同一 issue workspace。
- 旧进程未退出时，新进程不能进入该 issue 的 `before_run`、Codex run 或平台副作用。
- 父 Symphony 被 kill 但 Codex 子进程残留时，新实例不得进入同 workspace。
- snapshot 存在但 tracker 非 active 时，不自动运行且不自动清除，进入可诊断状态。
- 同一 issue 经崩溃、重启、retry fired、重复扫描后，不重复启动 worker，不重复进入同一 workspace 的 `before_run`/Codex。

## 正式文档更新位置

后续确认本草案后，应更新：

- `docs/方案/workflow-md-platform-adaptation.md`
  - 补充 continuation/resume 语义。
  - 明确 `retry_attempt` 和 `resume_reason`。
  - 明确 `Todo` 人工重新入队语义。
  - 明确 Orchestrator 不允许写 workflow label。

- `docs/方案/web-management-platform.md`
  - 移除或降级 `symphony-claimed` 作为状态源的表达。
  - 补充 service tree cleanup、DB lifecycle fencing/CAS、PID 验证、watcher lock、Web SIGTERM。
  - 明确 Web 不写 issue 级恢复摘要，第一阶段只缓存诊断。
  - 删除“未完成调度状态持久化到 SQLite”的旧表达，改为 rust-platform 本地诊断 snapshot + Web 项目级 process state。

- Rust implementation docs or comments
  - workspace metadata schema。
  - workspace identity / collision 处理。
  - legacy workspace 探测、迁移/alias/人工诊断策略。
  - resume snapshot schema。
  - RunContext Rust struct 与 Liquid 顶层变量 ABI。
  - retry/resume prompt 选择规则。
  - NoSlot/queue-delay 与 failure retry 的分离规则。
  - workspace eligibility 到 retry/claim 的映射。
  - snapshot 不记录 issue body、prompt、token 或敏感命令输出。

## 待决策问题

1. workspace 健康检查是否要求 repo 在 workspace 根，还是允许 `repo/` 子目录？推荐配置化，默认 workspace 根。
2. per-issue resume snapshot 是否需要同时生成全局聚合文件？推荐先不生成，避免单文件并发写复杂度。
3. `resume` JSON 是否需要加密？当前内容不应包含 token，不需要加密，但要避免记录 issue body/prompt/token。
4. auto-restart 达到上限后，active issue 是否自动告警？推荐告警但不改 tracker label。
5. Rework flow 是复用同一个 live workpad，还是删除/归档旧 workpad 后创建新 workpad？需要和“每 issue 一个 persistent workpad”规则统一。
6. 第二阶段是否恢复 retry due_at？如果恢复，必须先把它正式建模为负向调度状态源。
