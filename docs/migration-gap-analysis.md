# Symphony Rust 迁移差距分析文档

> 基于 SPEC.md 规范 + Elixir 参考实现，对比当前 Rust 实现的完整差距分析。
> 
> **v5 — 第四轮对抗验证修订** (2026-05-16)

---

## 一、总体评估

| 维度 | 状态 |
|------|------|
| 模块代码覆盖率 | ~70%（大部分模块已编写） |
| 集成串联完成度 | ~15%（模块间未连接） |
| 可运行完整度 | 不可用（main.rs 无法执行完整工作流） |

**核心问题**：各模块独立实现质量较高，但 `main.rs` 没有将它们串联成完整的运行时管线。当前启动后只能进入一个空的事件循环。

---

## 二、对抗验证结论

### 第一轮验证（v2）

两个独立 Agent 对本文档进行了交叉验证：

| 原始声明 | 验证结论 |
|---------|---------|
| 12 项严重差距中的 10 项 | **CONFIRMED** — 代码证据确认 |
| "Codex 无 turn/completed / turn/failed 事件处理" | **FALSE** — `check_turn_terminal()` 已正确处理 |
| "无续轮 continuation guidance" | **FALSE** — `PromptEngine::render()` 已支持 turn_number > 1 的续轮模板 |
| 6 项实现错误 | 全部 CONFIRMED |
| 正确实现清单 | 抽查 4 项全部确认正确 |

### 第二轮验证（v3）

#### Challenger（质疑者）验证结果

| 验证类别 | 数量 |
|---------|------|
| CONFIRMED（文档正确） | 28 |
| FALSE（文档有误） | 5 |
| PARTIAL（部分正确） | 6 |
| DOWNGRADED（严重度应降低） | 2 |
| 新发现问题 | 3 |

**关键误判修正**：

| 原始声明 | 验证结论 | 代码证据 |
|---------|---------|---------|
| "每轮后检查 issue 是否仍 active — 未实现" | **FALSE** — 已实现 | `agent/runner.rs:317-351` 通过 `state_refresher.refresh_issue_state()` 实现 |
| "Port 启动方式: 简化的 Command spawn" | **FALSE** — 已使用 `bash -lc` | `codex_client.rs:137-138` 使用 `Command::new("bash").args(["-lc", &codex_config.command])` |
| "turn_timeout_ms 未实现" | **FALSE** — 已实现 | `codex_client.rs:240-241` 使用 `tokio::time::timeout_at(turn_deadline, ...)` |
| "workspace/mod.rs 添加了非 SPEC 行为" | **FALSE** — 文件归属错误 | 非 SPEC 行为在 `service_config.rs:590-628`，`workspace/mod.rs:72-96` 是 SPEC 合规的 |
| "正确实现清单中 6 项实现错误全部 CONFIRMED" | **PARTIAL** — workspace 归属有误 | workspace sanitization 问题在 `service_config.rs` 而非 `workspace/mod.rs` |

#### Completeness Auditor（完整性审计）发现

发现 **16 项遗漏差距** + **4 项"正确实现"中的隐藏问题**：

**关键新发现**：
1. Codex Client 不传递 `cwd`/`approval_policy`/`sandbox` 给 app-server（严重）
2. Codex Client 不跨续轮复用 `thread_id`（严重）
3. `ConfigHolder` watcher 从未在 main.rs 中激活 — 热重载是死代码（中等 → 应从"正确实现"移除）
4. `DispatchConfig` 本身从未被更新 — `on_config_reloaded` 复制的是初始值（中等）
5. 三处重复的 `sanitize_workspace_key` 实现（中等）
6. `LiveSession.session_id` 永远是 "pending-0"，从未从 Codex 事件更新（低）
7. `max_turns=0` 和 `hooks.timeout_ms=0` 无验证（低）

---

## 三、按模块差距分析（已修订）

### 3.1 启动与配置加载 (`main.rs`)

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| 读取 WORKFLOW.md | 解析 YAML front matter + prompt body | 读取 `workflow.yaml`（遗留路径） | **严重** |
| 构造 ServiceConfig | `ServiceConfig::from_workflow()` | 未调用，手动构造 PlatformConfig | **严重** |
| 构造 PromptEngine | 编译 Liquid 模板 | 未构造 | **严重** |
| 构造 LinearClient | 连接 Linear GraphQL API | 未构造 | **严重** |
| 构造 WorkspaceManager | 管理工作目录生命周期 | 未构造 | **严重** |
| 构造 ToolRegistry | 注册动态工具 | 未构造 | **中等** |
| 启动时清理终态工作区 | `cleanup_terminal_workspaces()` | 未调用 | **中等** |
| HTTP Server 连接 | 通道连接到 Orchestrator | 通道创建但 `_rx` 未使用 | **中等** |
| GitHub adapter 支持 | 与 GitLab 同等 | 返回 "not yet available" 错误 | **中等** |
| ConfigHolder watcher 未激活 | SPEC 6.2: 必须检测 WORKFLOW.md 变更 | `ConfigHolder::with_watcher()` 存在但 main.rs 从未调用 | **中等** 🆕 |
| Orchestrator 使用默认 DispatchConfig | 应从配置文件派生 | `DispatchConfig::default()` 硬编码值，不读取配置 | **中等** 🆕 |

### 3.2 Orchestrator 事件循环 (`orchestrator/mod.rs`)

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| Tick 时拉取 Linear 候选 | 每次 tick 调用 tracker.fetch_candidates() | tick 只做 GC + 递增计数 + 检测 stall | **严重** |
| Tick 时调用 dispatch_candidates | 排序 + 过滤 + 派发 | `dispatch_candidates()` 方法存在但未被调用 | **严重** |
| 派发时 spawn AgentRunner | 创建 workspace → render prompt → 启动 Codex | 无 spawn 逻辑 | **严重** |
| RetryFired 重新派发 | 重新评估 issue 是否可派发 | 只移除 retry entry，不重新派发 | **严重** |
| 派发前重新验证 issue 状态 | `revalidate_issue_for_dispatch()` 防止 stale dispatch | 未实现 | **严重** ⬆️ |
| Reconciler Part B | 终态 issue 终止 worker + 清理 workspace（SPEC 8.5 每 tick **MUST** 执行） | 方法存在但未在 tick 中调用 | **严重** ⬆️ |
| Reconcile missing/invisible issues | 已删除/不可见 issue 终止 worker | 未实现 | **中等** 🆕 |
| Dispatch preflight validation | SPEC 6.3 **REQUIRED**: 每次 dispatch 前验证配置有效性 | 未实现 | **严重** 🆕 |
| 每次 tick 刷新配置 | 从 WORKFLOW.md 重新加载 | ConfigReloaded 事件只复制两个字段到 state，且 `DispatchConfig` 本身从未被更新（复制的是初始值） | **中等** ⬆️ |

### 3.3 Agent Runner / Worker 生命周期

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| AgentRunner 多轮循环 | 最多 max_turns 轮，每轮检查 issue 状态 | `agent/runner.rs` 存在但未连接 | **严重** |
| 首轮使用完整 prompt | render_prompt(issue, attempt) | PromptEngine 存在但未被调用 | **严重** |
| ~~续轮使用 continuation guidance~~ | ~~固定续轮提示~~ | ~~未实现~~ | ~~严重~~ → **已实现 ✓** |
| 续轮 guidance 内容规范 | SPEC 7.1 规定的多行续轮消息 | 使用简化的单行消息 | **中等** 🆕 |
| ~~每轮后检查 issue 是否仍 active~~ | ~~避免在终态 issue 上继续工作~~ | ~~未实现~~ | ~~中等~~ → **已实现 ✓**（`runner.rs:317-351` `state_refresher.refresh_issue_state()`） |
| Worker 运行时信息上报 | workspace_path, worker_host | 未实现 | **低** |

### 3.4 Codex 通信协议 (`codex_client.rs`)

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| JSON-RPC 2.0 握手 | initialize → thread/start → turn/start | 简化 JSON-line 协议，无标准握手 | **严重** |
| 自动审批 (approval) | 对 commandExecution/fileChange 自动 approve（SPEC 10.5: **MUST NOT** leave stalled） | 未实现 — approval 请求被忽略，导致 session stall 直到 turn_timeout | **严重** ⬆️ |
| 动态工具调用 | item/tool/call → DynamicTool.execute | 未实现 | **严重** |
| 条件审批策略 | `approval_policy == "never"` 时才自动审批 | 未实现 | **中等** 🆕 |
| 用户输入请求处理 | requestUserInput → 解析 questions/options → 选择审批选项或非交互回答 | 未实现 | **中等** ⬆️ |
| ~~turn/completed 事件~~ | ~~正常结束信号~~ | ~~未处理~~ | ~~严重~~ → **已实现 ✓** |
| ~~turn/failed 事件~~ | ~~异常结束信号~~ | ~~未处理~~ | ~~严重~~ → **已实现 ✓** |
| ~~Port 启动方式~~ | ~~`bash -lc <codex.command>`~~ | ~~简化的 Command spawn~~ | ~~中等~~ → **已实现 ✓**（`codex_client.rs:137-138` 已使用 `bash -lc`） |
| Codex 不传递 cwd/approval/sandbox 给 app-server | turn/start 需携带 workspace cwd、approval_policy、sandbox 设置 | `send_turn_start()` 只发送 prompt，不传递任何配置 | **严重** 🆕 |
| Codex 不跨续轮复用 thread_id | 同一 worker run 内所有续轮使用同一 thread_id | `thread_id` 从事件提取但从未回传到后续 turn 请求 | **严重** 🆕 |
| read_timeout_ms | 握手阶段超时（防止 hung startup） | 字段存在但未用于任何超时逻辑 | **中等** ⬆️ |
| ~~turn_timeout_ms~~ | ~~整轮超时~~ | ~~未实现~~ | ~~低~~ → **已实现 ✓**（`codex_client.rs:240-241` `timeout_at`） |
| `stop` 消息非标准协议 | 关闭应遵循目标协议规范 | 发送 `{"type": "stop"}` 非文档化消息 | **低** 🆕 |
| 不传递 issue metadata 作为 session title | SPEC 10.2: 支持 turn/session title 时应包含 issue.identifier + title | `send_turn_start()` 无 title/metadata 字段 | **低** 🆕 |

### 3.5 Prompt 渲染 (`prompt/mod.rs`)

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| Liquid 模板编译 | 支持 issue + attempt 变量 | 已实现 ✓ | 无 |
| Strict 模式 | 未知变量/过滤器报错（SPEC 5.4 **MUST**） | 未强制 strict mode，未知变量渲染为空字符串 | **严重** ⬆️ |
| DateTime 转 ISO-8601 | 日期类型序列化 | 未明确处理 | **低** |
| 空模板 fallback | 使用 Elixir 定义的默认 prompt 模板 | 未实现 | **低** |

### 3.6 Tracker / Linear 客户端 (`tracker/linear.rs`)

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| fetch_candidate_issues | GraphQL 分页查询 + **服务端状态过滤** | 查询无 state filter，全量拉取后客户端过滤 | **中等** ⬆️ |
| fetch_issues_by_states | 按状态批量查询 | 已实现 ✓ | 无 |
| fetch_issue_states_by_ids | 按 ID 批量查状态 | 已实现 ✓ | 无 |
| Assignee 路由 ("me" 解析) | viewer query 获取当前用户 + 按 assignee 过滤派发 | 完全未实现（无 viewer query、无 assignee_id 字段、无路由过滤） | **严重** ⬆️ |
| Blocker 提取 | inverseRelations type=blocks | 已实现 ✓ | 无 |

### 3.7 Workspace 管理 (`workspace/mod.rs`)

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| 目录创建 | 在 root 下按 identifier 创建 | 已实现 ✓ | 无 |
| 标识符清理 | `[^a-zA-Z0-9._-]` → `_`（仅替换） | `workspace/mod.rs:72-96` 是 SPEC 合规的；**但 `service_config.rs:590-628` 有非 SPEC 行为**（字符白名单不含 dot、下划线合并、首尾裁剪） | **中等**（归属修正：问题在 `service_config.rs`） |
| 三处重复实现 | 应只有一处权威实现 | `workspace/mod.rs:72`、`models/mod.rs:366`、`service_config.rs:590` 三处独立实现 | **中等** 🆕 |
| 路径安全 | 禁止 symlink 逃逸 | 已实现 ✓ | 无 |
| 生命周期 hooks | after_create, before_run, after_run, before_remove | 已实现 ✓ | 无 |
| Hook 超时 | 可配置 timeout_ms | 已实现 ✓ | 无 |
| Hook timeout_ms=0 无验证 | SPEC: "Invalid values fail configuration validation" | 接受 0 值，导致 hook 立即超时 | **低** 🆕 |
| 启动时终态清理 | 清理已完成 issue 的工作区 | 方法存在但未在 main 中调用 | **中等** |
| 关闭时 after_run hook | SPEC 9.4: 取消时也应执行 after_run | `worker_handle.abort()` 时 future 被 drop，hook 不执行 | **中等** 🆕 |

### 3.8 Platform Adapter (`platform/`)

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| GitLab adapter | Issue/Label/Comment/MR 操作 | 已实现 ✓ | 无 |
| GitHub adapter | Issue/Label/Comment/PR 操作 | 代码存在但 main.rs 拒绝启动 | **中等** |
| CooldownQueue | API 限流保护 | 已实现 ✓ | 无 |
| MemoryAdapter | 测试用内存实现 | 已实现 ✓ | 无 |

### 3.9 调度器 (`orchestrator/scheduler.rs`)

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| 7 条派发规则 | 字段/状态/running/claimed/全局并发/状态并发/blocker | 已实现 ✓ | 无 |
| SSH worker host 容量检查 | `worker_slots_available?` — 至少一台 host 有空位 | 未实现（缺第 8 条规则） | **中等** 🆕 |
| Assignee 路由检查 | `issue_routable_to_worker?` — 只派发分配给本 worker 的 issue | 未实现 | **严重** 🆕 |
| 优先级排序 | priority ASC → created_at ASC → identifier ASC | 已实现 ✓ | 无 |
| Blocker 检查 | todo 状态 blocker 必须全部终态 | 已实现 ✓ | 无 |

### 3.10 重试机制 (`orchestrator/retry.rs`)

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| 指数退避公式 | `min(10000 * 2^(attempt-1), max_backoff)` | 已实现 ✓ | 无 |
| Continuation retry | 正常退出后 1000ms | 已实现 ✓ | 无 |
| 退避上限 cap | power 10 (1024s ≈ 17min) | 已实现 ✓ | 无 |
| RetryFired 后重新派发 | 重新评估 + dispatch | **未实现**（只移除 entry） | **严重** |
| Retry fetch 失败时重新调度 | 拉取失败 → 重新 schedule_retry(attempt+1) | 未实现 | **中等** 🆕 |

### 3.11 Stall 检测 (`orchestrator/reconciler.rs`)

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| 活动超时检测 | last_activity > stall_timeout | 已实现 ✓ | 无 |
| 两阶段取消 | cancel signal → 30s hard kill | 已实现 ✓ | 无 |
| Reconciler Part B | 终态 issue 终止 worker（SPEC 8.5 每 tick MUST 执行） | 方法存在但未调用 | **严重** ⬆️ |

### 3.12 SSH Worker Host Pool 🆕

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| SSH 执行模块 | 通过 SSH 在远程 host 执行命令 | 完全不存在 | **中等** |
| Host 选择策略 | least-loaded 选择 + per-host 容量限制 | 不存在 | **中等** |
| 远程 workspace 操作 | SSH 上创建目录/执行 hooks | 不存在 | **中等** |
| Host 亲和性 | retry 时优先使用同一 host | 不存在 | **低** |
| IPv6 + host:port 解析 | 支持 `[::1]:22` 和 `host:port` 格式 | 不存在 | **低** |

> 注：SSH Worker 在 SPEC 中标记为 OPTIONAL 扩展，非核心功能。

### 3.13 配置类型与验证 🆕

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| `approval_policy` | 支持 String 或 Map（含 reject 规则） | `Option<String>` — 无法表示 Map | **中等** |
| `turn_sandbox_policy` | 动态构造 Map（基于 workspace path） | `Option<String>` — 无法表示动态 Map；项目 WORKFLOW.md 中定义为 map，会被静默丢弃 | **中等** ⬆️ |
| `max_turns` 正整数验证 | SPEC 5.3.5: "positive integer"，无效值应报错 | 接受 0 值（u64），导致 turn loop 执行一轮后退出（turn_number=1 >= max_turns=0） | **中等** 🆕 |
| `hooks.timeout_ms` 验证 | SPEC 5.3.4: "Invalid values fail configuration validation" | 接受 0 值，导致 hook 立即超时；非数字值静默回退默认值 | **低** 🆕 |

### 3.14 日志与可观测性 (`logging.rs`)

| 项目 | SPEC/Elixir 要求 | 当前 Rust 实现 | 差距等级 |
|------|-----------------|---------------|---------|
| 结构化日志 | tracing with issue_id, session_id | 已实现 ✓ | 无 |
| Token 计量 | 绝对值 + delta 追踪 | 已实现 ✓ | 无 |
| Rate-limit 提取 | 从 Codex 事件解析 rate_limit 结构 | 字段存在但无解析逻辑 | **低** 🆕 |
| turn_count 递增 | session_started 事件时递增 | 字段存在但无递增逻辑，HTTP API 永远显示 0 | **低** 🆕 |
| session_id 从未更新 | 应从 Codex thread_id + turn_id 组合 | `LiveSession` 初始化为 "pending-0"，`on_codex_update()` 从未更新 session_id/thread_id/turn_id | **低** 🆕 |

---

## 四、按严重程度汇总（v5 修订）

### 严重（阻塞运行 / SPEC MUST 不合规）— 17 项（v3: 14 项，提升 3 项）

1. `main.rs` 不读取 WORKFLOW.md
2. `main.rs` 不构造 ServiceConfig / PromptEngine / LinearClient / WorkspaceManager
3. Orchestrator tick 不拉取 Linear 候选
4. Orchestrator tick 不调用 dispatch_candidates
5. 派发时不 spawn AgentRunner
6. RetryFired 不重新派发
7. AgentRunner 未连接到 Orchestrator
8. Codex 通信无 JSON-RPC 2.0 握手（含 initialize → thread/start → turn/start 三阶段）
9. Codex 无自动审批处理（SPEC 10.5 MUST NOT stall — 当前会 stall 直到 turn_timeout）⬆️
10. Codex 无动态工具调用
11. 派发前不重新验证 issue 状态（会导致 stale dispatch）
12. Assignee 路由完全缺失（多实例部署时会派发错误 issue；*单实例可降为中等*）
13. Codex 不传递 cwd/approval_policy/sandbox 给 app-server（turn 请求只含 prompt）
14. Codex 不跨续轮复用 thread_id（每次 turn 独立，无法保持上下文）
15. **Prompt strict mode 未强制**（SPEC 5.4 MUST: 未知变量必须报错）⬆️ 从中等提升
16. **Reconciler Part B 未调用**（SPEC 8.5 MUST: 每 tick 必须执行两部分 reconciliation）⬆️ 从中等提升
17. **Dispatch preflight validation 未执行**（SPEC 6.3 REQUIRED）🆕

### 中等（功能缺失但不阻塞编译）— 22 项（v3: 22 项，移除 2 项提升为严重，新增 2 项）

1. GitHub adapter 在 main.rs 中被禁用
2. ~~Reconciler Part B 未调用~~ → 提升为严重（v4）
3. HTTP Server 通道未连接
4. 启动时终态工作区清理未调用
5. 配置热重载只更新部分字段（且 `DispatchConfig` 本身从未被更新）
6. ~~Prompt strict mode 未强制~~ → 提升为严重（v4）
7. ~~每轮后检查 issue 状态~~ → **已实现 ✓**（v3 移除）
8. ToolRegistry 未注册到 CodexClient
9. ~~Worker 运行时信息上报~~ → 降为低
10. Linear candidate query 无服务端 state 过滤
11. Workspace 标识符清理：`service_config.rs:590` 有非 SPEC 行为（归属修正）
12. Reconcile missing/invisible running issues
13. 续轮 guidance 内容不符合 SPEC 7.1 规范
14. SSH worker host 容量检查缺失
15. 条件审批策略 (approval_policy map)
16. turn_sandbox_policy 动态构造（项目 WORKFLOW.md 的 map 值会被静默丢弃）
17. requestUserInput question/answer 协议
18. Retry fetch 失败时重新调度
19. ConfigHolder watcher 从未在 main.rs 激活 — 热重载是死代码
20. Orchestrator 使用 `DispatchConfig::default()` 硬编码值
21. 三处重复 `sanitize_workspace_key` 实现（维护隐患）
22. 关闭时 `worker_handle.abort()` 不执行 after_run hook
23. `read_timeout_ms` 字段存在但未用于启动超时（hung startup 会阻塞整个 turn_timeout）
24. `max_turns=0` 无验证（导致 turn loop 执行一轮后退出）
25. **HTTP API 不返回 workspace 字段** 🆕
26. **两个同名但不兼容的 `CodexEventUpdate` 结构体（`codex_client.rs:85` vs `models/mod.rs:295`）** 🆕

### 低（细节/优化）— 10 项（v3: 8 项，新增 2 项）

1. DateTime 转 ISO-8601
2. 空模板 fallback（需复制 Elixir 默认模板内容）
3. ~~read_timeout_ms / turn_timeout_ms~~ → `turn_timeout_ms` 已实现，`read_timeout_ms` 提升为中等
4. ~~Codex 启动方式 `bash -lc`~~ → **已实现 ✓**（v3 移除）
5. Rate-limit 提取逻辑
6. turn_count 递增逻辑（HTTP API 永远显示 0）
7. SSH host 亲和性 + IPv6 解析
8. `LiveSession.session_id` 永远是 "pending-0"，从未从 Codex 事件更新
9. Codex `stop` 消息 `{"type": "stop"}` 非标准协议
10. `hooks.timeout_ms=0` 无验证（hook 立即超时）
11. **不传递 issue metadata 作为 session title** 🆕
12. **Worker 运行时信息上报（workspace_path, worker_host）** 🆕

---

## 五、建议迁移顺序（v5 修订）

### Phase 1: 核心管线串联（使系统可运行）

```
main.rs 重写:
  WorkflowLoader::load("WORKFLOW.md")
  → ServiceConfig::from_workflow()
  → PromptEngine::new(prompt_template)
  → LinearClient::new(config.linear)
  → WorkspaceManager::new(config.workspace)
  → ConfigHolder::with_watcher()          ← v3 新增：激活热重载
  → DispatchConfig::from_service_config()  ← v3 新增：从配置派生
  → Orchestrator::new(all_deps)
  → orchestrator.run()
```

**预计工作量**: 2-3 天

**完成标志**: `cargo run -- --workflow WORKFLOW.md` 成功加载配置，进入 Orchestrator 事件循环，日志输出 tick 心跳。系统可启动但不会派发任何工作。

### Phase 2: Orchestrator 集成（使调度可工作）

1. Tick 中调用 LinearClient.fetch_candidates()
2. **添加 per-tick dispatch preflight validation（SPEC 6.3 REQUIRED）** 🆕
3. 调用 dispatch_candidates() 进行排序+过滤
4. 添加 revalidate_issue_for_dispatch() 防止 stale dispatch
5. 添加 assignee 路由过滤
6. 派发时 spawn AgentRunner task
7. RetryFired 重新评估+派发（含 fetch 失败重新调度）
8. Reconciler Part B 接入 + missing issue reconciliation
9. `on_config_reloaded()` 更新完整 `DispatchConfig`（不只是两个字段）
10. **Prompt strict mode 启用（SPEC 5.4 MUST）** 🆕
    > ⚠️ `liquid` crate 0.26 **不支持** `strict_variables(true)` API。需要替代方案：
    > (a) 渲染前提取模板变量名，与 context 对比，缺失则报错；
    > (b) 使用 `liquid-core` 底层 API 自定义变量解析器；
    > (c) 评估切换到支持 strict mode 的模板引擎（如 `tera`）。
    > 选择方案后预计增加 0.5-1 天工作量。

**预计工作量**: 5-6 天（因新增 MUST 项上调）

**完成标志**: Orchestrator 每次 tick 从 Linear（或 mock tracker）拉取候选 issue，满足条件时 spawn AgentRunner task。使用 mock Codex 可完成端到端 dispatch → run → complete 流程。

> **Phase 间依赖说明**：步骤 6 "spawn AgentRunner" 依赖 Phase 3 的 Codex 协议实现。Phase 2 完成时可使用 mock Codex 进行集成测试，Phase 3 完成后切换为真实协议。

### Phase 3: Codex 协议实现（使 Agent 可执行）

> **前置参考**：Codex app-server 协议文档为实现的 source of truth。
> 运行 `codex app-server generate-json-schema --out <dir>` 获取 `v2/ThreadStartParams.json` 和 `v2/TurnStartParams.json` 的完整 schema。
> SPEC Section 5.3.6 + 10.1-10.5 定义了 Symphony 侧的集成要求。

1. JSON-RPC 2.0 完整握手 (initialize → thread/start → turn/start)
2. **turn/start 传递 cwd、approval_policy、sandbox 配置** 🆕
3. **续轮复用 thread_id（同一 worker run 内保持上下文）** 🆕
4. 事件流解析 (approval requests, tool calls, user input)
5. 条件审批逻辑（基于 approval_policy 值）— Phase 3 先实现自动审批，操作员介入见 Phase 6
6. requestUserInput 协议处理 — Phase 3 先实现 fail-turn 策略，操作员介入见 Phase 6
7. 动态工具调用分发
8. 多轮循环 + SPEC 7.1 规范的 continuation guidance 内容

**预计工作量**: 5-7 天（因新增协议复杂度上调）

**完成标志**: AgentRunner 能与真实 Codex CLI 完成一次完整的 JSON-RPC 2.0 握手 + 单轮 turn + approval 自动处理 + 正常退出。续轮能复用 thread_id。

### Phase 4: 补全与加固

1. GitHub adapter 启用
2. HTTP Server 连接
3. ~~Strict prompt mode~~ → 已移至 Phase 2
4. 启动清理
5. 配置热重载完善（ConfigHolder watcher 激活）
6. Linear query 添加服务端 state 过滤
7. Workspace sanitization：统一为单一实现，移除 `service_config.rs` 中的非 SPEC 行为
8. approval_policy / turn_sandbox_policy 类型修正为 serde_json::Value
9. `max_turns` 正整数验证 + `hooks.timeout_ms` 边界验证
10. 关闭时确保 after_run hook 执行（用 `abort_handle` + cleanup 替代直接 abort）
11. `LiveSession` 从 Codex 事件更新 session_id/thread_id/turn_id
12. **统一 `CodexEventUpdate` 结构体（消除 `codex_client.rs` 与 `models/mod.rs` 的重复定义）** 🆕

**预计工作量**: 4-5 天

**完成标志**: 所有 SPEC MUST 合规性检查表（Section 八）中的 8 项不合规变为合规。`cargo test` 全量通过。

### Phase 5: 可选扩展（SSH Worker）

1. SSH 执行模块
2. Host 选择策略 (least-loaded)
3. Per-host 容量限制
4. 远程 workspace 操作
5. Host 亲和性

**预计工作量**: 3-4 天（OPTIONAL，可延后）

### Phase 6: 操作员介入模式（TODO）

> **TODO**: 本 Phase 实现操作员实时介入能力，使 approval 和 user-input 场景支持人工决策。
> Phase 3 先以"全自动审批 + user-input fail-turn"策略达成 SPEC 合规，本 Phase 在此基础上扩展。

1. **Approval 操作员介入**
   - HTTP API 暴露 pending approval 队列（`GET /approvals/pending`）
   - 操作员通过 API 手动 approve/reject（`POST /approvals/:id/resolve`）
   - 可配置超时（如 `operator_timeout_ms`，超时后按 `approval_policy` 自动处理）
   - WebSocket/SSE 推送 approval 事件到操作员前端

2. **User-Input 操作员介入**
   - Codex 发出 requestUserInput 时，暂停 turn（不立即 fail）
   - HTTP API 暴露 pending questions（`GET /inputs/pending`）
   - 操作员通过 API 提交回答（`POST /inputs/:id/answer`）
   - 可配置超时（超时后 fail turn + retry）
   - 支持 questions/options 结构化展示

3. **操作员通知机制**
   - Webhook 回调（配置 `operator.webhook_url`）
   - 可选：Slack/飞书/钉钉集成
   - 通知内容：issue identifier + 请求类型 + 上下文摘要

4. **策略配置扩展**
   ```yaml
   # WORKFLOW.md 中的配置示例
   codex:
     approval_policy:
       default: auto          # 默认自动审批
       on_reject_match: ask   # 匹配 reject 规则时询问操作员
     user_input_policy: ask   # "ask" = 等待操作员, "fail" = 立即失败
     operator_timeout_ms: 300000  # 5 分钟无响应则按 fallback 处理
   ```

5. **状态机扩展**
   - Turn 新增 `waiting_for_operator` 状态
   - 该状态下 stall_timeout 暂停计时
   - 操作员响应后恢复 turn 执行

**预计工作量**: 5-7 天（OPTIONAL，依赖 Phase 3 完成）

**Phase 1-4 总计**: 16-23 天（含测试编写，建议每 Phase 增加 30-50% buffer）

### 实施风险提示

| 风险等级 | 修复项 | 风险描述 |
|---------|--------|---------|
| 高 | Phase 1: main.rs 重写 | 替换启动路径后所有现有 e2e 测试将失败，需同步更新 |
| 高 | Phase 3: Codex 协议重写 | `CodexClient` 的 `send_turn_start()` 和 `stream_turn_events()` 需完全重写，影响 mock_codex 测试 |
| 高 | Phase 4: 统一 sanitize_workspace_key | 已存在的 workspace 目录名可能与新逻辑不匹配（迁移兼容性） |
| 中 | Phase 2: dispatch_candidates 改为 async | 当前是同步方法，改为 async 需修改所有调用方和测试 |
| 中 | Phase 2: Reconciler Part B | 可能误杀正在运行的 worker（如果 tracker 返回 stale 状态数据） |
| 中 | Phase 4: approval_policy 类型变更 | `CodexConfig` 公共字段变更，影响所有使用方 |

---

## 六、经验证的模块代码（v5 修订）

以下模块经对比验证，**代码逻辑正确**（但部分未在运行时激活，标注为"死代码"）：

**可达模块**（从 main.rs 可达）：
- [x] 调度器 7 条规则 + 优先级排序 ⚠️ *缺第 8 条 SSH host 规则和 assignee 路由；全局 slots 检查未计入 retry 中的 claimed issues*
- [x] 指数退避计算 + continuation retry
- [x] Stall 检测两阶段取消
- [x] Linear GraphQL 客户端（3 个操作 + 分页 + 归一化）⚠️ *candidate query 缺服务端 state 过滤*
- [x] GitLab adapter 全功能
- [x] CooldownQueue 限流
- [x] MemoryAdapter 测试适配器
- [x] 结构化日志 + Token 计量
- [x] 优雅关闭序列 ⚠️ *abort 时不保证 after_run hook 执行*

**死代码模块**（从 main.rs 不可达，修复 Phase 1 后即可激活）：
- [x] ~~配置热重载机制（ArcSwap + notify）~~ → ⚠️ `ConfigHolder::with_watcher()` 从未在 main.rs 中调用
- [x] WORKFLOW.md 前置 YAML 解析 → ⚠️ main.rs 不使用 WorkflowLoader
- [x] Liquid 模板编译与渲染（含续轮支持）→ ⚠️ main.rs/orchestrator 不调用 PromptEngine；strict mode 未强制
- [x] Workspace 管理（创建/hooks/清理/路径安全）→ ⚠️ 仅被 AgentRunner 使用，从 main.rs 不可达；`workspace/mod.rs` 本身 SPEC 合规，问题在 `service_config.rs`
- [x] **AgentRunner 及其所有内部功能** → ⚠️ 从未被 orchestrator spawn，以下均不可达：
  - turn/completed + turn/failed 事件处理（`codex_client.rs:401-449`）
  - 每轮后 issue 状态检查（`runner.rs:317-351` `state_refresher`）
  - turn_timeout_ms 超时机制（`codex_client.rs:240-250`）
  - `bash -lc` 启动方式（`codex_client.rs:137-138`）

**注**：`CodexClient` 仅被 `AgentRunner` 使用，因此其所有内部功能（超时、事件处理、启动方式）均为死代码。

---

## 七、实现错误清单（v5 修订）

| 位置 | 问题 | 影响 |
|------|------|------|
| `main.rs:load_config_from_args()` | 读取 `workflow.yaml` 而非 `WORKFLOW.md` | 整个配置链路断裂 |
| `main.rs` 整体 | 不调用 `ServiceConfig`/`ConfigHolder`/`WorkflowLoader`/`PromptEngine`/`WorkspaceManager`/`AgentRunner` | 所有核心模块为死代码 |
| `orchestrator/mod.rs::on_tick()` | 不执行 dispatch preflight validation（SPEC 6.3 REQUIRED） | 无法保证配置有效性 |
| `orchestrator/mod.rs::on_tick()` | 不调用 reconciler Part B（SPEC 8.5 MUST） | 终态 issue 的 worker 永远不会被终止 |
| `orchestrator/mod.rs::on_retry_fired()` | 只移除 retry entry，不重新评估派发 | 重试永远不会实际执行 |
| `orchestrator/mod.rs::on_config_reloaded()` | 只复制 `poll_interval_ms` 和 `max_concurrent_agents` 到 state，但 `DispatchConfig` 本身从未被更新 | 复制的是初始硬编码值，配置变更实际不生效 |
| `codex_client.rs::send_turn_start()` | 只发送 `{"type": "turn.start", "prompt": ...}`，不传递 cwd/approval/sandbox | Codex 无法知道工作目录和审批策略 |
| `codex_client.rs::run_turn()` | 不回传 thread_id 到后续 turn 请求 | 续轮无法保持上下文 |
| `main.rs::build_adapter()` | GitHub kind 返回错误 | GitHub 用户无法使用 |
| `service_config.rs:590-628` ⚠️ 归属修正 | `sanitize_workspace_key()` 字符白名单不含 dot（`.`）+ 下划线合并 + 首尾裁剪 | `issue.1.2` → `issue_1_2`（SPEC 应保留为 `issue.1.2`），破坏迁移兼容性 |
| `tracker/linear.rs::fetch_candidate_issues()` | GraphQL query 无 state 过滤变量 | 大项目性能问题，拉取全量 issue |
| `service_config.rs::CodexConfig` | `approval_policy` 和 `turn_sandbox_policy` 类型为 `Option<String>` | 无法表示 Map 值；项目 WORKFLOW.md 的 map 配置会被静默丢弃 |
| `prompt/mod.rs` | `ParserBuilder::with_stdlib()` 未启用 strict mode | SPEC 5.4 MUST: 未知变量渲染为空字符串而非报错 |
| `codex_client.rs:85` + `models/mod.rs:295` 🆕 | 两个同名但不兼容的 `CodexEventUpdate` 结构体 | 模块断裂的具体表现，集成时需统一 |
| `orchestrator/mod.rs:225` | `LiveSession::new("pending", "0")` 后 session_id/thread_id/turn_id 从未更新 | 可观测性数据永远是占位值 |
| `models/mod.rs:120` | `turn_count: 0` 从未递增 | HTTP API 状态永远显示 0 轮 |

---

## 八、SPEC MUST 合规性检查（v5 修订）

以下为 SPEC 中使用 MUST/REQUIRED 关键字的核心要求合规状态：

| SPEC 条款 | MUST 要求 | 合规状态 |
|-----------|-----------|---------|
| 5.2 | YAML front matter MUST decode to a map/object | ✅ 合规 — `workflow_loader` 检查 |
| 5.4 | Unknown variables MUST fail rendering | ❌ 不合规 — liquid 默认不 strict |
| 5.4 | Unknown filters MUST fail rendering | ❌ 不合规 — 同上 |
| 6.2 | MUST detect WORKFLOW.md changes | ❌ 不合规 — watcher 代码存在但未激活 |
| 6.2 | MUST re-read and re-apply without restart | ❌ 不合规 — 同上 |
| 6.2 | Invalid reloads MUST NOT crash the service | ✅⚠️ 代码合规但为死代码 — `try_reload()` 错误被 catch，但 watcher 从未激活 |
| 6.3 | Validate configuration before starting scheduling loop | ❌ 不合规 — 使用 legacy config 路径 |
| 7.4 | `claimed` and `running` checks REQUIRED before launching worker | ✅ 合规 — `should_dispatch()` 检查 |
| 8.2 | Issue must have id, identifier, title, state | ✅ 合规 — `should_dispatch()` 检查 |
| 8.5 | Reconciliation MUST execute both parts each tick | ❌ 不合规 — Part B 未调用 |
| 9.5 | Workspace path MUST stay inside workspace root | ✅⚠️ 代码合规但为死代码 — `validate_path_containment()` 仅被 AgentRunner 使用 |
| 9.5 | Coding-agent cwd MUST be per-issue workspace path | ✅⚠️ 代码合规但为死代码 — `CodexClient::start()` 设置 `current_dir` |
| 9.5 | Workspace directory names MUST use sanitized identifiers | ✅⚠️ 代码合规但为死代码 — `sanitize_workspace_key()` |
| 10.1 | Launched process MUST speak compatible app-server protocol | ❌ 不合规 — 使用简化 JSON-line |
| 10.5 | Approval requests MUST NOT leave a run stalled indefinitely | ❌ 不合规 — 无 approval 处理 |
| 11.2 | Pagination REQUIRED for candidate issues | ✅ 合规 — `fetch_issues_paginated()` |
| 13.1 | REQUIRED context fields: issue_id, issue_identifier | ✅ 合规 — tracing spans 包含 |

**MUST 合规率**: 5/17 = 29% 完全合规（可运行路径），4 项代码合规但为死代码，8 项不合规

---

## 九、验证方法论

本文档经过以下对抗验证流程：

### 第一轮（v1 → v2）

1. **初始分析** — 基于 SPEC.md + Elixir 源码 + Rust 源码生成差距列表
2. **Challenger 验证** — 独立 Agent 逐条阅读 Rust 源码，确认/否定每项声明
3. **Completeness Audit** — 独立 Agent 对照 SPEC 全文 + Elixir 全部模块，寻找遗漏
4. **修订整合** — 移除误判（2 项），提升严重度（3 项），新增遗漏（16 项）

### 第二轮（v2 → v3）

1. **Challenger 验证** — 逐条验证 v2 文档所有声明（28 CONFIRMED / 5 FALSE / 6 PARTIAL）
2. **Completeness Audit** — 深度审计发现 16 项遗漏 + 4 项"正确实现"中的隐藏问题
3. **修订整合**：
   - 移除误判 3 项（post-turn check、bash -lc、turn_timeout_ms 均已实现）
   - 修正归属 1 项（workspace sanitization 问题在 `service_config.rs` 非 `workspace/mod.rs`）
   - 新增严重差距 2 项（Codex 不传配置、不复用 thread_id）
   - 新增中等差距 6 项（ConfigHolder 死代码、DispatchConfig 硬编码、三处重复实现、abort 不执行 hook、read_timeout_ms 提升、max_turns 验证）
   - 新增低等差距 3 项（session_id 占位、stop 消息非标准、hooks.timeout_ms 验证）
   - 重构"正确实现清单"为"经验证的模块代码"，标注死代码状态

### 第三轮（v3 → v4）

1. **Challenger 验证** — 行号精确性验证 + 死代码标注一致性 + 描述准确性
2. **Completeness Audit** — 端到端场景走查 + SPEC MUST 合规性系统检查 + 测试覆盖分析
3. **修订整合**：
   - 提升严重度 3 项（Prompt strict mode、Reconciler Part B、approval stall — 均为 SPEC MUST）
   - 新增严重差距 1 项（dispatch preflight validation）
   - 新增中等差距 2 项（HTTP API workspace 字段、CodexEventUpdate 重复定义）
   - 新增低等差距 2 项（issue metadata title、Worker 运行时信息）
   - 修正描述精确性（`service_config.rs` dot 字符差异、`max_turns=0` 行为）
   - 统一死代码标注（CodexClient 所有内部功能归入 AgentRunner 死代码组）
   - 新增 SPEC MUST 合规性检查表（Section 八）
### 第四轮（v4 → v5）

1. **Challenger 验证** — Section 间交叉一致性 + MUST 合规表算术验证 + 计数精确性 + Phase 覆盖完整性
2. **Completeness Audit** — 实施可行性评估 + 风险矩阵 + 验收标准 + 非功能性差距 + 文档结构优化
3. **修订整合**：
   - 修正合规率算术（10/17 → 5/17 完全合规 + 4 项死代码合规）
   - 修正中等计数（21 → 22 项）
   - 修正 Section 3.11 Reconciler Part B 严重度（中等 → 严重）
   - 修正 Section 六 Workspace 分类（可达 → 死代码）
   - Strict prompt mode 从 Phase 4 提前到 Phase 2（SPEC MUST）
   - Phase 2 新增 dispatch preflight validation 步骤
   - 新增实施风险提示表
   - 新增 Phase 间依赖说明
   - 标注 SPEC 9.5 合规项为"代码合规但为死代码"

---

*文档生成时间: 2026-05-16*
*对比基准: SPEC.md (2170行) + Elixir 参考实现 (symphony-main/elixir/)*
*验证状态: v5 — 第四轮对抗验证修订*
*SPEC MUST 合规率: 29% 完全合规 (5/17)，24% 代码合规但死代码 (4/17)，47% 不合规 (8/17)*
