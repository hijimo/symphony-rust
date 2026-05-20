# Symphony 业务文档

## 1. 产品定位

Symphony 是一个自动化编码代理编排平台，将 Issue Tracker 中的任务自动分配给 AI 编码代理（Codex），在隔离工作空间中完成编码工作。它将人工触发的编码流程转变为可重复的守护进程工作流。

### 核心价值

- **自动化调度**：持续轮询 Issue Tracker，自动发现并分配待处理任务
- **隔离执行**：每个 Issue 在独立工作空间中运行，互不干扰
- **策略即代码**：工作流配置（`WORKFLOW.md`）随代码版本管理，团队可自定义 Agent 行为
- **运维可观测**：结构化日志 + HTTP Dashboard，实时监控多个并发 Agent 运行状态

## 2. 业务流程

### 2.1 整体工作流

```
┌─────────────┐     轮询      ┌──────────────┐    调度     ┌─────────────┐
│ Issue Tracker│◄────────────►│  Orchestrator │──────────►│ Agent Runner │
│ (Linear/     │              │  (编排器)      │            │ (Codex)     │
│  GitHub/     │              └──────────────┘            └─────────────┘
│  GitLab)     │                     │                          │
└─────────────┘                     │                          │
                                    ▼                          ▼
                            ┌──────────────┐          ┌─────────────┐
                            │ Retry Queue  │          │  Workspace  │
                            │ (重试队列)    │          │ (隔离目录)   │
                            └──────────────┘          └─────────────┘
```

### 2.2 Issue 生命周期

1. **发现**：编排器按固定间隔轮询 Tracker，获取处于活跃状态（如 Todo、In Progress）的 Issue
2. **调度**：按优先级排序，检查并发限制和阻塞依赖，选择可执行的 Issue
3. **执行**：为 Issue 创建/复用工作空间，渲染 Prompt 模板，启动 Codex Agent 会话
4. **多轮对话**：Agent 在工作空间中执行编码任务，支持多轮交互直到完成或达到轮次上限
5. **退出处理**：
   - 正常退出 → 短延迟后续行（Continuation），重新检查 Issue 状态
   - 异常退出 → 指数退避重试
   - Issue 进入终态 → 释放资源，清理工作空间
6. **协调**：持续监控运行中的 Agent，检测停滞、处理状态变更

### 2.3 调度策略

- **优先级排序**：priority 数值越小优先级越高，相同优先级按创建时间排序
- **并发控制**：全局最大并发数 + 按状态的并发上限
- **阻塞检查**：Issue 有未完成的阻塞依赖时不会被调度
- **去重**：已在运行/重试/已完成的 Issue 不会重复调度

### 2.4 容错机制

| 场景 | 处理方式 |
|------|----------|
| Agent 异常退出 | 指数退避重试（基础 1s，上限可配置，默认 5 分钟） |
| Agent 正常退出但 Issue 未完成 | 1s 延迟后续行，重新检查状态 |
| Agent 停滞（无活动） | 停滞检测超时后终止并重试 |
| Issue 状态变为终态 | 取消运行中的 Agent，释放资源 |
| Issue 从 Tracker 消失 | 协调器检测后终止对应 Agent |
| 配置文件变更 | 热重载，无需重启服务 |
| 服务收到终止信号 | 优雅关闭：停止调度 → 等待 Worker 完成 → 退出 |

## 3. 支持的平台

### 3.1 Tracker（任务来源）

| 平台 | 功能 | 说明 |
|------|------|------|
| Linear | 完整支持 | GraphQL API，支持分页、优先级、阻塞关系 |
| GitHub | Issues 作为 Tracker | 通过 label 管理工作流状态 |
| GitLab | Issues 作为 Tracker | 通过 label 管理工作流状态 |

### 3.2 Platform（代码托管 & 操作）

| 平台 | 功能 |
|------|------|
| GitHub | Issue CRUD、Label 管理、评论、PR 创建、凭证验证 |
| GitLab | Issue CRUD、Label 管理、评论、PR 创建、凭证验证 |

### 3.3 Agent 后端

| 后端 | 协议 |
|------|------|
| Codex app-server | JSON-line stdio 协议，支持多轮对话 |

## 4. 配置体系

### 4.1 WORKFLOW.md

Symphony 的核心配置文件，采用 YAML Front Matter + Markdown 正文的格式：

- **Front Matter**：运行时配置（Tracker 连接、轮询间隔、并发限制、Hook 脚本等）
- **Markdown 正文**：Prompt 模板，使用 Liquid 模板语法

### 4.2 配置分层

```
WORKFLOW.md (团队策略，版本管理)
    ├── tracker: 任务来源配置
    ├── polling: 轮询策略
    ├── agent: 并发与重试策略
    ├── codex: Agent 进程配置
    ├── workspace: 工作空间配置
    ├── hooks: 生命周期脚本
    └── prompt: Agent 指令模板
```

### 4.3 环境变量

敏感信息（API Key、Token）通过 `$ENV_VAR` 语法引用环境变量，不直接写入配置文件。

### 4.4 热重载

配置文件变更时自动重载，以下配置可运行时生效：
- 轮询间隔
- 并发限制
- 重试退避上限
- Hook 超时
- 按状态并发上限

## 5. 工作空间管理

### 5.1 目录结构

```
{workspace_root}/
├── {issue-identifier-1}/    # 如 PROJ-42/
│   └── (git clone 内容)
├── {issue-identifier-2}/
└── ...
```

### 5.2 生命周期 Hook

| Hook | 触发时机 | 失败影响 | 典型用途 |
|------|----------|----------|----------|
| `after_create` | 工作空间首次创建后 | 回滚创建 | `git clone $REPO_URL .` |
| `before_run` | 每次 Agent 运行前 | 中止本次运行 | `git pull origin main` |
| `after_run` | Agent 运行结束后 | 忽略 | 清理临时文件 |
| `before_remove` | 工作空间删除前 | 忽略 | 备份数据 |

## 6. 可观测性

### 6.1 HTTP Dashboard

启用 `--port` 参数后提供：

- **Web Dashboard**（`GET /`）：实时展示所有运行中/重试中的 Agent 状态
- **系统状态 API**（`GET /api/v1/state`）：JSON 格式的完整运行时状态
- **Issue 详情**（`GET /api/v1/{identifier}`）：单个 Issue 的执行详情
- **手动刷新**（`POST /api/v1/refresh`）：触发立即轮询

### 6.2 监控指标

- 运行中 Agent 数量与并发槽位使用率
- Token 消耗统计（输入/输出/总计）
- 累计运行时间
- 重试队列深度
- Rate Limit 状态

### 6.3 结构化日志

使用 tracing 框架输出 JSON 格式日志，支持通过 `RUST_LOG` 环境变量控制日志级别。

## 7. 安全边界

- Symphony 是调度器/运行器，不直接修改 Issue 状态或创建 PR
- Issue 的状态变更、评论、PR 操作由 Agent 通过工具完成
- 工作空间路径经过安全校验，防止路径穿越
- API Key 通过环境变量注入，不持久化到磁盘
- 适用于受信任环境，不提供多租户隔离

## 8. 典型部署场景

1. **单仓库自动化**：一个 Symphony 实例对应一个代码仓库，处理该仓库的所有 Issue
2. **多仓库编排**：多个 Symphony 实例各自管理不同仓库
3. **CI/CD 集成**：作为长驻服务运行，配合 Git Hook 实现自动化编码流水线
