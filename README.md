# Symphony

Symphony 是一个 Rust 实现的自动化编码代理编排平台。它从任务追踪器（Linear/GitHub/GitLab）中拉取 Issue，自动调度 AI 编码代理（Codex）在隔离工作空间中完成编码任务。

> [!WARNING]
> Symphony 是一个工程预览版本，适用于受信任的环境中测试。

## 功能特性

- 多平台支持：Linear（Tracker）、GitLab/GitHub（Platform Adapter）
- 事件驱动编排器：优先级调度、并发控制、阻塞依赖检查
- 容错机制：指数退避重试、停滞检测、优雅关闭
- 隔离工作空间：每个 Issue 独立目录，支持生命周期 Hook
- Liquid 模板引擎：灵活的 Prompt 构建
- 配置热重载：运行时更新配置无需重启
- HTTP Dashboard：实时监控运行状态和 Token 消耗

## 快速开始

### 环境要求

- Rust 1.70+
- Codex CLI（作为 AI Agent 后端）
- Linear API Key 或 GitHub/GitLab Token

### 编译

```bash
cd rust-platform
cargo build --release
```

### 配置

创建 `WORKFLOW.md` 文件：

```markdown
---
tracker:
  kind: linear
  api_key: $LINEAR_API_KEY
  project_slug: my-project
  active_states: [Todo, In Progress]
  terminal_states: [Done, Closed, Cancelled]
polling:
  interval_ms: 30000
agent:
  max_concurrent_agents: 10
  max_turns: 20
workspace:
  root: ~/symphony_workspaces
hooks:
  after_create: "git clone $REPO_URL ."
  before_run: "git pull origin main"
codex:
  command: "codex app-server"
  turn_timeout_ms: 3600000
  stall_timeout_ms: 300000
server:
  port: 8080
---
你是一个编码助手，正在处理 Issue {{ issue.identifier }}: {{ issue.title }}

{{ issue.description }}

请在分支 {{ issue.branch_name }} 上完成此任务。
```

### 设置环境变量

```bash
export LINEAR_API_KEY="lin_api_xxxxx"
# 或者
export GITHUB_TOKEN="ghp_xxxxx"
export GITLAB_TOKEN="glpat-xxxxx"

# 测试用（集成测试/E2E 测试需要）
export TEST_REPO_NAME="owner/repo"          # GitHub 和 GitLab 共用仓库名
export GITLAB_BASE_URL="https://gitlab.com" # GitLab 实例地址（默认 gitlab.com）
```

### 运行

```bash
# 使用默认 ./WORKFLOW.md
./target/release/symphony-platform

# 指定配置文件路径
./target/release/symphony-platform /path/to/WORKFLOW.md

# 启用 HTTP Dashboard
./target/release/symphony-platform --port 8080
```

## 配置参考

### tracker 配置

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `kind` | string | `linear` | 追踪器类型：`linear`/`github`/`gitlab` |
| `api_key` | string | - | API 密钥，支持 `$ENV_VAR` 引用 |
| `endpoint` | string | 按 kind 自动选择 | API 端点 URL |
| `project_slug` | string | - | Linear 项目 slug（Linear 必填） |
| `active_states` | list | `[Todo, In Progress]` | 活跃状态列表 |
| `terminal_states` | list | `[Done, Closed, Cancelled, ...]` | 终态列表 |

### agent 配置

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `max_concurrent_agents` | int | `10` | 最大并发 Agent 数 |
| `max_turns` | int | `20` | 单次运行最大轮次 |
| `max_retry_backoff_ms` | int | `300000` | 最大重试退避时间（ms） |
| `blocker_check_states` | list | `[todo]` | 需要检查阻塞依赖的状态 |
| `max_concurrent_agents_by_state` | map | `{}` | 按状态的并发上限 |

### codex 配置

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `command` | string | `codex app-server` | Codex 启动命令 |
| `turn_timeout_ms` | int | `3600000` | 单轮超时（1 小时） |
| `read_timeout_ms` | int | `5000` | 读取超时 |
| `stall_timeout_ms` | int | `300000` | 停滞检测超时（5 分钟） |

### hooks 配置

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `after_create` | string | - | 工作空间创建后执行（失败则回滚） |
| `before_run` | string | - | Agent 运行前执行（失败则中止） |
| `after_run` | string | - | Agent 运行后执行（失败忽略） |
| `before_remove` | string | - | 工作空间删除前执行（失败忽略） |
| `timeout_ms` | int | `60000` | Hook 执行超时 |

### Prompt 模板变量

| 变量 | 类型 | 说明 |
|------|------|------|
| `issue.id` | string | Issue 内部 ID |
| `issue.identifier` | string | 人类可读标识（如 `PROJ-42`） |
| `issue.title` | string | 标题 |
| `issue.description` | string/nil | 描述 |
| `issue.priority` | int/nil | 优先级（数字越小越高） |
| `issue.state` | string | 当前状态 |
| `issue.branch_name` | string/nil | 关联分支名 |
| `issue.url` | string/nil | Issue URL |
| `issue.labels` | array | 标签列表（小写） |
| `issue.blocked_by` | array | 阻塞依赖列表 |
| `attempt` | int/nil | 重试次数（首次为 nil） |
| `turn_number` | int | 当前轮次 |
| `max_turns` | int | 最大轮次 |
| `is_continuation` | bool | 是否为续行轮次 |

## HTTP API

启用 `--port` 后可访问：

- `GET /` — HTML Dashboard，实时展示运行状态
- `GET /api/v1/state` — JSON 格式系统状态
- `GET /api/v1/{identifier}` — 单个 Issue 详情
- `POST /api/v1/refresh` — 触发立即轮询

## 开发

### 运行测试

```bash
cd rust-platform
cargo test
```

### 项目结构

```
rust-platform/src/
├── main.rs          # 入口
├── cli.rs           # CLI 参数
├── config/          # 配置加载与校验
├── orchestrator/    # 核心编排器
├── platform/        # GitHub/GitLab 适配器
├── tracker/         # Linear 客户端
├── agent/           # Codex 进程管理
├── workspace/       # 工作空间管理
├── prompt/          # 模板引擎
├── server/          # HTTP API
└── tools/           # 工具集成
```

## 文档

- [业务文档](docs/business.md) — 业务流程与功能说明
- [架构文档](docs/architecture.md) — 技术架构与设计决策
- [设计文档](docs/rust-rewrite-design.md) — Rust 重写设计方案

## 许可证

Apache License 2.0 — 详见 [LICENSE](LICENSE)
