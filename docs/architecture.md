# Symphony 架构文档

## 1. 系统架构概览

Symphony 是一个单二进制 CLI 应用，基于 Tokio 异步运行时构建，采用事件驱动的状态机架构。

```
┌─────────────────────────────────────────────────────────────────────┐
│                         Symphony Platform                            │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌──────────┐   ┌──────────────────────────────────────────────┐    │
│  │ CLI      │   │            Orchestrator (事件循环)             │    │
│  │ (clap)   │──►│  ┌──────────┐ ┌───────────┐ ┌───────────┐   │    │
│  └──────────┘   │  │Scheduler │ │Reconciler │ │  Retry    │   │    │
│                  │  │(调度器)   │ │(协调器)    │ │ (重试队列) │   │    │
│  ┌──────────┐   │  └──────────┘ └───────────┘ └───────────┘   │    │
│  │ Config   │   └──────────────────────────────────────────────┘    │
│  │ Watcher  │          │              │              │               │
│  │(热重载)   │          ▼              ▼              ▼               │
│  └──────────┘   ┌──────────┐   ┌──────────┐   ┌──────────┐         │
│                  │ Tracker  │   │ Agent    │   │Workspace │         │
│  ┌──────────┐   │ Client   │   │ Runner   │   │ Manager  │         │
│  │ HTTP     │   │(Linear/  │   │(Codex)   │   │(目录管理) │         │
│  │ Server   │   │ GH/GL)   │   └──────────┘   └──────────┘         │
│  │(Axum)    │   └──────────┘        │                               │
│  └──────────┘                       ▼                               │
│                              ┌──────────┐                           │
│                              │ Prompt   │                           │
│                              │ Engine   │                           │
│                              │(Liquid)  │                           │
│                              └──────────┘                           │
└─────────────────────────────────────────────────────────────────────┘
```

## 2. 分层架构

### 2.1 层次划分

| 层 | 职责 | 模块 |
|----|------|------|
| Policy Layer | 团队策略定义 | `WORKFLOW.md` |
| Configuration Layer | 配置解析、校验、热重载 | `config/` |
| Coordination Layer | 调度、并发控制、重试、协调 | `orchestrator/` |
| Execution Layer | 工作空间管理、Agent 进程管理 | `workspace/`, `agent/` |
| Integration Layer | 外部平台适配 | `tracker/`, `platform/` |
| Observability Layer | 日志、HTTP API | `logging/`, `server/` |

### 2.2 依赖方向

```
Policy → Configuration → Coordination → Execution → Integration
                                    ↘ Observability
```

各层单向依赖，不存在循环引用。

## 3. 核心组件设计

### 3.1 Orchestrator（编排器）

编排器是系统的核心，采用单线程事件循环模式，所有状态变更通过事件通道序列化。

**事件类型**：

| 事件 | 来源 | 处理 |
|------|------|------|
| `Tick` | 定时器 | 触发轮询、协调、调度 |
| `WorkerExitNormal` | Worker 任务 | 续行调度 |
| `WorkerExitAbnormal` | Worker 任务 | 指数退避重试 |
| `CodexUpdate` | Agent 事件流 | 更新 Token 计数、活动时间 |
| `RetryFired` | 重试定时器 | 重新调度 Issue |
| `ConfigReloaded` | 文件监听器 | 更新运行时配置 |
| `ForceRefresh` | HTTP API | 立即触发轮询 |
| `QueryState` | HTTP API | 返回状态快照 |
| `Shutdown` | 信号处理器 | 优雅关闭 |

**状态机**：

```
                    ┌─────────┐
                    │  Idle   │◄──────────────────────┐
                    └────┬────┘                       │
                         │ Tick                       │
                         ▼                            │
                    ┌─────────┐                       │
                    │  Poll   │                       │
                    └────┬────┘                       │
                         │                            │
                         ▼                            │
              ┌──────────────────┐                    │
              │  Reconcile       │                    │
              │  (停滞检测+终态)  │                    │
              └────────┬─────────┘                    │
                       │                              │
                       ▼                              │
              ┌──────────────────┐                    │
              │  Dispatch        │                    │
              │  (调度新任务)     │────────────────────┘
              └──────────────────┘
```

### 3.2 Agent Runner（Agent 运行器）

每个 Issue 的执行由独立的 Tokio 任务承载：

```
spawn_worker(issue)
    │
    ├── 1. 创建/复用工作空间
    ├── 2. 执行 before_run Hook
    ├── 3. 启动 Codex app-server 子进程
    ├── 4. 多轮循环:
    │       ├── 渲染 Prompt（首轮/续行模板）
    │       ├── 发送 Turn 请求
    │       ├── 流式接收事件 → 转发给 Orchestrator
    │       ├── 检查 Issue 状态是否仍活跃
    │       └── 检查轮次上限
    ├── 5. 停止 Codex 会话
    ├── 6. 执行 after_run Hook
    └── 7. 报告退出事件给 Orchestrator
```

### 3.3 Codex Client（Codex 协议客户端）

与 Codex app-server 通过 stdio JSON-line 协议通信：

- **启动**：`bash -lc "{command}"` 在工作空间目录中启动子进程
- **Thread Start**：发送线程初始化参数（sandbox 策略、approval 策略）
- **Turn Start**：发送 Prompt，开始一轮对话
- **事件流**：逐行读取 stdout，解析 JSON 事件（token 使用、完成通知等）
- **停止**：发送 stop 命令，超时后强制 kill

### 3.4 Config Watcher（配置热重载）

```
WORKFLOW.md 文件变更
    │ (notify crate 监听)
    ▼
解析新配置 → 校验 → arc-swap 原子替换 → 通知 Orchestrator
```

使用 `arc-swap` 实现无锁配置读取，配置消费者始终读到一致的快照。

### 3.5 Tracker Client（任务追踪器客户端）

**Tracker Trait 接口**：

```rust
trait Tracker {
    async fn fetch_candidates() -> Vec<TrackerIssue>;     // 获取活跃 Issue
    async fn fetch_by_states(states) -> Vec<TrackerIssue>; // 按状态查询
    async fn fetch_by_ids(ids) -> Vec<TrackerIssue>;       // 按 ID 查询
}
```

**适配器实现**：
- `LinearTracker`：直接实现 Tracker trait，使用 GraphQL API
- `GitLabTracker`：桥接 Platform trait 到 Tracker trait
- `GitHubTracker`：桥接 Platform trait 到 Tracker trait

### 3.6 Platform（代码托管平台）

**Platform Trait 接口**：

```rust
trait Platform {
    async fn list_issues(state) -> Vec<PlatformIssue>;
    async fn get_issue(id) -> PlatformIssue;
    async fn create_comment(issue_id, body);
    async fn add_label(issue_id, label);
    async fn remove_label(issue_id, label);
    async fn create_pull_request(...);
    async fn validate_credentials();
}
```

GitHub 和 GitLab 各自实现此 trait，通过 label 模拟工作流状态。

## 4. 数据流

### 4.1 调度数据流

```
Tracker API → fetch_candidates() → Vec<TrackerIssue>
    → 转换为 Vec<Issue>
    → 过滤已 claimed/completed
    → 检查阻塞依赖
    → 检查并发槽位
    → 按优先级排序
    → 取前 N 个调度
```

### 4.2 事件数据流

```
Codex stdout → JSON parse → CodexEventUpdate
    → mpsc channel → Orchestrator
    → 更新 LiveSession (token 计数、活动时间)
    → 更新 CodexTotals (全局统计)
```

### 4.3 状态查询数据流

```
HTTP Request → Axum Handler
    → oneshot channel → Orchestrator
    → 构建 StateResponse 快照
    → oneshot reply → HTTP Response (JSON)
```

## 5. 并发模型

### 5.1 任务结构

```
main task
├── Orchestrator event loop (单线程，所有状态变更序列化)
├── HTTP Server (Axum, 多连接并发)
├── Config Watcher (文件系统监听)
├── Signal Handler (SIGINT/SIGTERM)
└── Worker tasks (每个 Issue 一个 Tokio task)
    └── Codex subprocess (子进程 + stdio 流)
```

### 5.2 同步机制

| 机制 | 用途 |
|------|------|
| `mpsc` channel | Worker → Orchestrator 事件通信 |
| `oneshot` channel | HTTP API → Orchestrator 状态查询 |
| `arc-swap` | 配置热重载（无锁读） |
| `CancellationToken` | 优雅取消 Worker 任务 |

### 5.3 并发控制

- **全局并发上限**：`max_concurrent_agents`，限制同时运行的 Worker 数量
- **按状态并发上限**：`max_concurrent_agents_by_state`，限制特定状态的并发数
- **Claimed 集合**：防止同一 Issue 被重复调度（running + retrying）

## 6. 启动流程

```
1.  解析 CLI 参数 (clap)
2.  初始化日志 (tracing-subscriber)
3.  解析 Workflow 路径
4.  加载 WORKFLOW.md → WorkflowDefinition
5.  构建 ServiceConfig (类型化配置 + 环境变量解析)
6.  校验配置完整性
7.  编译 Prompt 模板 (Liquid)
8.  构建 Tracker Client
9.  构建 WorkspaceManager + 启动清理
10. 激活 ConfigHolder (文件监听 + arc-swap)
11. 派生 DispatchConfig
12. 注册信号处理 (SIGINT/SIGTERM)
13. 构建 Orchestrator
14. 启动 HTTP Server (可选)
15. 进入事件循环 (orchestrator.run())
16. 优雅关闭
```

## 7. 关键设计决策

### 7.1 单线程事件循环

所有编排状态变更通过单一事件循环序列化，避免锁竞争和数据竞态。Worker 通过 channel 异步报告事件，编排器按序处理。

**优势**：状态一致性保证简单、调试容易、无死锁风险

### 7.2 无持久化状态

编排器状态完全在内存中，重启后通过重新轮询 Tracker 恢复。不依赖数据库或持久化队列。

**优势**：部署简单、无外部依赖、状态始终与 Tracker 一致
**代价**：重启后丢失重试计数和运行时统计

### 7.3 WORKFLOW.md 作为单一配置源

所有运行时行为由仓库内的 `WORKFLOW.md` 定义，包括 Prompt、调度策略、Hook 脚本。

**优势**：配置版本化、团队可审查、环境一致性

### 7.4 Workspace 持久化

工作空间目录在 Issue 完成前保留，支持跨重试复用。避免每次重试都重新 clone 仓库。

### 7.5 协调器双阶段协调

- **Part A（停滞检测）**：基于单调时钟检测无活动的 Agent，超时后终止
- **Part B（终态协调）**：查询 Tracker 确认 Issue 是否已进入终态，终止不再需要的 Agent

## 8. 技术栈

| 类别 | 技术 | 用途 |
|------|------|------|
| 语言 | Rust | 系统编程、内存安全 |
| 异步运行时 | Tokio | 异步 I/O、任务调度 |
| HTTP 客户端 | Reqwest | Tracker/Platform API 调用 |
| HTTP 服务器 | Axum | Dashboard & API |
| CLI | Clap | 命令行参数解析 |
| 模板引擎 | Liquid | Prompt 渲染 |
| 配置格式 | YAML (serde_yaml) | WORKFLOW.md Front Matter |
| 文件监听 | Notify | 配置热重载 |
| 原子配置 | Arc-swap | 无锁配置读取 |
| 日志 | Tracing + tracing-subscriber | 结构化日志 |
| 时间 | Chrono | UTC 时间处理 |
| 序列化 | Serde + serde_json | 数据序列化 |
| 错误处理 | Thiserror | 类型化错误 |
| 测试 | Wiremock, Mockall, Tempfile | HTTP Mock、Trait Mock、临时文件 |

## 9. 模块依赖图

```
main.rs
├── cli (Clap 参数定义)
├── config/
│   ├── workflow_loader (WORKFLOW.md 解析)
│   ├── service_config (类型化配置)
│   ├── watcher (热重载)
│   ├── validator (校验)
│   └── platform (平台配置结构)
├── orchestrator/
│   ├── scheduler (调度逻辑)
│   ├── reconciler (协调逻辑)
│   └── retry (重试计算)
├── agent/
│   ├── runner (Worker 生命周期)
│   └── codex_client (Codex 协议)
├── tracker/
│   ├── linear (Linear GraphQL)
│   └── gitlab (GitLab 桥接)
├── platform/
│   ├── github (GitHub REST)
│   ├── gitlab (GitLab REST)
│   ├── http_client (带重试的 HTTP)
│   └── cooldown_queue (限流队列)
├── workspace/ (目录生命周期)
├── prompt/ (Liquid 模板)
├── server/
│   └── api (Axum 路由)
├── tools/
│   ├── linear_graphql (Agent 工具)
│   └── platform_api (Agent 工具)
├── models/ (领域模型)
├── error (错误类型)
└── logging/ (日志初始化)
```

## 10. 扩展点

| 扩展方向 | 实现方式 |
|----------|----------|
| 新增 Tracker | 实现 `Tracker` trait |
| 新增 Platform | 实现 `Platform` trait |
| 新增 Agent 后端 | 替换 `codex_client` 实现 |
| 自定义调度策略 | 修改 `orchestrator/scheduler.rs` |
| 自定义 Hook | 在 WORKFLOW.md 中配置 shell 脚本 |
| 自定义 Prompt | 修改 WORKFLOW.md 模板正文 |
