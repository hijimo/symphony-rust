# Symphony 业务文档

## 1. 项目概述

Symphony 是一个自动化编码代理编排平台，用于将项目管理工具（如 Linear、GitHub Issues、GitLab Issues）中的任务自动分配给 AI 编码代理（Codex）执行。系统通过轮询任务追踪器获取待处理的 Issue，自动调度 AI 代理在隔离的工作空间中完成编码任务，并管理整个生命周期。

## 2. 核心业务流程

### 2.1 整体工作流

```
任务追踪器 (Linear/GitHub/GitLab)
        │
        ▼
  ┌─────────────┐
  │  轮询获取    │  每 30s（可配置）拉取活跃状态的 Issue
  │  候选 Issue  │
  └──────┬──────┘
         │
         ▼
  ┌─────────────┐
  │  调度决策    │  优先级排序 → 并发控制 → 阻塞检查
  └──────┬──────┘
         │
         ▼
  ┌─────────────┐
  │  工作空间    │  创建/复用隔离目录 → 执行 Hook 脚本
  │  准备       │
  └──────┬──────┘
         │
         ▼
  ┌─────────────┐
  │  Agent 执行  │  启动 Codex → 构建 Prompt → 多轮对话
  └──────┬──────┘
         │
         ▼
  ┌─────────────┐
  │  结果处理    │  正常退出 → 续行重试 / 异常退出 → 指数退避重试
  └─────────────┘
```

### 2.2 Issue 生命周期状态机

```
  Todo ──────────────► In Progress ──────────────► Done
   │                       │                        ▲
   │                       │                        │
   │ (被阻塞时暂停调度)     │ (异常退出)              │ (正常完成)
   │                       ▼                        │
   │                  Retry Queue ──────────────────┘
   │                       │
   │                       │ (达到终态)
   ▼                       ▼
 Cancelled / Closed / Duplicate
```

## 3. 核心业务能力

### 3.1 多平台支持

| 平台 | 角色 | 状态 |
|------|------|------|
| Linear | Issue Tracker（GraphQL API） | 已实现 |
| GitLab | Platform Adapter（Issue/PR/Label 操作） | 已实现 |
| GitHub | Platform Adapter（Issue/PR/Label 操作） | 接口已定义 |

### 3.2 智能调度

- **优先级排序**：priority 数值越小优先级越高，None 排最后
- **时间排序**：同优先级按创建时间升序（先创建先处理）
- **全局并发控制**：限制同时运行的 Agent 总数（默认 10）
- **按状态并发控制**：可为不同状态设置独立并发上限
- **阻塞依赖检查**：Todo 状态的 Issue 如果有未完成的 Blocker 则跳过

### 3.3 容错与重试

- **正常退出续行**：Agent 正常完成一轮后，1 秒后重新检查 Issue 状态并决定是否继续
- **异常退出重试**：指数退避重试（attempt 递增，最大退避时间可配置）
- **停滞检测**：通过单调时钟检测 Agent 无活动超时，自动取消并重试
- **硬截止时间**：取消信号发出后 30 秒仍未退出则强制终止

### 3.4 工作空间管理

- 每个 Issue 拥有独立的工作目录（基于 identifier 生成安全目录名）
- 支持生命周期 Hook：`after_create`、`before_run`、`after_run`、`before_remove`
- 路径安全校验：防止路径穿越攻击
- 终态清理：Issue 进入终态后自动清理工作空间

### 3.5 Prompt 模板引擎

- 基于 Liquid 模板语法，严格模式
- 支持主模板（首轮）和续行模板（后续轮次）
- 模板变量：`issue`（完整 Issue 对象）、`attempt`、`turn_number`、`max_turns`、`is_continuation`

### 3.6 可观测性

- 结构化日志（tracing + JSON 格式）
- HTTP Dashboard（实时查看运行状态、重试队列、Token 消耗）
- Token 用量追踪（input/output/total tokens）
- Rate Limit 监控

## 4. 配置方式

系统通过 `WORKFLOW.md` 文件配置，采用 YAML Front Matter + Markdown Body 格式：

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
---
你是一个编码助手，正在处理 Issue {{ issue.identifier }}: {{ issue.title }}

{{ issue.description }}
```

## 5. 业务约束

- 所有 API Token 通过环境变量注入（`$VAR` 语法），不在配置文件中明文存储
- 工作空间目录名经过安全清洗，防止路径注入
- Agent 进程以独立进程组运行，支持干净的信号传播
- 优雅关闭：收到 SIGINT/SIGTERM 后取消所有 Worker，等待超时后强制终止
- 配置热重载：支持运行时更新配置而无需重启

## 6. 目标用户

- 使用 Linear/GitHub/GitLab 管理项目的开发团队
- 希望将重复性编码任务自动化的工程团队
- 需要批量处理 Issue 的 DevOps/Platform 团队
