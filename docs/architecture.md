# Symphony 项目架构文档

## 1. 技术栈

| 类别 | 技术选型 |
|------|---------|
| 语言 | Rust 2021 Edition |
| 异步运行时 | Tokio (full features) |
| HTTP 客户端 | reqwest (rustls-tls) |
| HTTP 服务端 | Axum 0.8 |
| 序列化 | serde + serde_json + serde_yaml |
| 模板引擎 | Liquid 0.26 |
| 文件监控 | notify 6 |
| CLI 解析 | clap 4 (derive) |
| 日志 | tracing + tracing-subscriber (JSON) |
| 错误处理 | thiserror 2 |
| 并发原语 | tokio::sync, dashmap, arc-swap |
| 进程管理 | tokio::process + libc (SIGKILL) |

## 2. 项目结构

```
symphony-rust/
├── rust-platform/              # 主 Rust 项目
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs             # 入口：CLI 解析 → 日志 → 配置 → 启动
│   │   ├── lib.rs              # 模块导出
│   │   ├── cli.rs              # CLI 参数定义
│   │   ├── error.rs            # 统一错误类型 PlatformError
│   │   ├── logging/mod.rs      # 结构化日志初始化
│   │   ├── models/mod.rs       # 核心数据模型
│   │   ├── config/             # 配置层
│   │   │   ├── mod.rs
│   │   │   ├── platform.rs     # 平台配置结构体 (Config, PlatformConfig)
│   │   │   ├── service_config.rs # 运行时配置 (ServiceConfig)
│   │   │   ├── workflow_loader.rs # WORKFLOW.md 解析器
│   │   │   ├── validator.rs    # 配置校验
│   │   │   └── watcher.rs      # 配置热重载 (notify + arc-swap)
│   │   ├── orchestrator/       # 编排器（核心状态机）
│   │   │   ├── mod.rs          # Orchestrator 事件循环
│   │   │   ├── scheduler.rs    # 调度逻辑（eligibility + sort + dispatch）
│   │   │   ├── reconciler.rs   # 停滞检测 + 状态协调
│   │   │   └── retry.rs        # 重试队列（指数退避）
│   │   ├── platform/           # 平台适配器层
│   │   │   ├── mod.rs          # Platform trait 定义
│   │   │   ├── issue.rs        # Issue/PR/Comment 数据模型
│   │   │   ├── github.rs       # GitHub 适配器
│   │   │   ├── gitlab.rs       # GitLab 适配器
│   │   │   ├── memory.rs       # 内存适配器（测试用）
│   │   │   ├── http_client.rs  # HTTP 客户端封装
│   │   │   ├── retry.rs        # HTTP 重试策略
│   │   │   ├── cooldown_queue.rs # 冷却队列（防止重复处理）
│   │   │   └── workflow.rs     # 工作流状态操作
│   │   ├── tracker/            # Issue Tracker 客户端
│   │   │   ├── mod.rs          # Tracker trait 定义
│   │   │   └── linear.rs       # Linear GraphQL 客户端
│   │   ├── agent/              # AI Agent 管理
│   │   │   ├── mod.rs
│   │   │   ├── codex_client.rs # Codex 子进程管理（stdio JSON-line 协议）
│   │   │   └── runner.rs       # Worker 完整生命周期
│   │   ├── workspace/mod.rs    # 工作空间管理
│   │   ├── prompt/mod.rs       # Liquid 模板引擎
│   │   ├── server/             # HTTP 服务扩展
│   │   │   ├── mod.rs
│   │   │   └── api.rs          # REST API + Dashboard
│   │   └── tools/              # 工具集成
│   │       ├── mod.rs
│   │       ├── platform_api.rs # Platform API 工具
│   │       └── linear_graphql.rs # Linear GraphQL 工具
│   └── tests/                  # 测试
│       ├── unit/               # 单元测试
│       ├── integration/        # 集成测试
│       ├── e2e/                # 端到端测试
│       ├── common/             # 测试公共设施
│       └── fixtures/           # 测试数据
├── docs/                       # 文档
├── SPEC.md                     # 完整规格说明
├── WORKFLOW.md                  # 工作流配置示例
└── LICENSE
```

## 3. 核心架构设计

### 3.1 事件驱动架构

Orchestrator 采用单线程事件循环模式，通过 `tokio::sync::mpsc` 通道串行处理所有事件，避免并发状态竞争：

```
                    ┌──────────────────────────────────┐
                    │         Orchestrator             │
                    │   (单一权威运行时状态)             │
                    │                                  │
  Tick ────────────►│  OrchestratorState {             │
  WorkerExitNormal─►│    running: HashMap<id, Entry>   │
  WorkerExitAbnorm─►│    claimed: HashSet<id>          │
  CodexUpdate ─────►│    retry_attempts: HashMap       │
  RetryFired ──────►│    completed: HashMap            │
  ConfigReloaded ──►│    codex_totals: TokenTotals     │
  ForceRefresh ────►│  }                               │
  Shutdown ────────►│                                  │
                    └──────────────────────────────────┘
```

### 3.2 分层架构

```
┌─────────────────────────────────────────────────────────┐
│                    CLI / HTTP Server                      │  入口层
├─────────────────────────────────────────────────────────┤
│                     Orchestrator                          │  编排层
│  (事件循环 + 调度 + 重试 + 协调)                          │
├─────────────────────────────────────────────────────────┤
│          Agent Runner          │     Workspace Manager    │  执行层
│  (Worker 生命周期管理)          │  (目录 + Hook 管理)      │
├─────────────────────────────────────────────────────────┤
│  Codex Client  │  Prompt Engine  │  Config Watcher        │  基础设施层
│  (子进程通信)   │  (Liquid 模板)   │  (热重载)             │
├─────────────────────────────────────────────────────────┤
│     Tracker (Linear)     │     Platform (GitLab/GitHub)   │  外部集成层
│  (GraphQL 分页查询)       │  (REST API 适配)              │
└─────────────────────────────────────────────────────────┘
```

### 3.3 关键设计决策

#### 单一状态所有权
Orchestrator 拥有唯一的 `OrchestratorState`，所有状态变更通过事件通道串行执行，无需锁。

#### 进程组隔离
Codex 子进程通过 `process_group(0)` 创建独立进程组，确保 SIGKILL 能传播到所有子进程。

#### 单调时钟停滞检测
使用 `std::time::Instant`（单调时钟）而非系统时钟检测 Agent 停滞，避免 NTP 时间跳变导致误判。

#### 配置热重载
通过 `notify` 监控 WORKFLOW.md 文件变更，使用 `arc-swap` 原子替换配置快照，Agent 在启动时快照配置实现隔离。

#### Claim-then-Register 两阶段调度
1. `claim_issue`：将 Issue ID 加入 claimed 集合（防止重复调度）
2. `register_running`：Worker 实际启动后注册到 running 表

## 4. 通信协议

### 4.1 Codex JSON-Line 协议

Symphony 与 Codex app-server 通过 stdio 使用 JSON-Line 协议通信：

**请求（stdin → Codex）：**
```json
{"type": "turn.start", "prompt": "..."}
{"type": "stop"}
```

**响应（Codex → stdout）：**
```json
{"type": "turn.completed", "usage": {"input_tokens": 100, "output_tokens": 50}}
{"type": "turn.failed", "error": "reason"}
{"type": "turn.cancelled"}
{"type": "turn.input_required"}
```

### 4.2 HTTP API

| 端点 | 方法 | 说明 |
|------|------|------|
| `/` | GET | HTML Dashboard |
| `/api/v1/state` | GET | 系统状态 JSON |
| `/api/v1/{identifier}` | GET | 单个 Issue 详情 |
| `/api/v1/refresh` | POST | 触发立即轮询 |

## 5. 错误处理策略

### 5.1 错误分类

| 错误类型 | 处理方式 |
|---------|---------|
| 可重试（Timeout/5xx/RateLimit） | 指数退避重试 |
| 认证失败（401/403） | 记录错误，不重试 |
| 配置错误 | 启动时校验失败，拒绝启动 |
| Hook 失败 | `after_create`/`before_run` 致命；`after_run`/`before_remove` 忽略 |
| Agent 停滞 | 取消 → 30s 硬截止 → 强制终止 → 重试 |

### 5.2 优雅关闭流程

1. 设置 `shutting_down` 标志
2. 取消所有重试定时器
3. 中止 Tick 定时器
4. 向所有 Worker 发送取消信号
5. 等待 Worker 退出（带超时）
6. 超时后强制终止剩余 Worker
7. 清空 claimed 集合

## 6. 可扩展性设计

- **Tracker trait**：新增 Issue Tracker 只需实现 `fetch_candidate_issues`、`fetch_issues_by_states`、`fetch_issue_states_by_ids` 三个方法
- **Platform trait**：新增代码托管平台需实现 Issue/PR/Label/Comment 操作接口
- **Hook 系统**：通过 Shell 脚本扩展工作空间生命周期行为
- **模板引擎**：Liquid 模板支持自定义 Prompt 逻辑
- **HTTP 扩展**：Axum Router 可方便添加新端点
