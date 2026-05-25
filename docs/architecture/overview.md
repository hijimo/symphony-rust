# Symphony 系统架构概览

## 1. 系统定位与目标

Symphony 是一个自动化编码代理编排平台，将 Issue Tracker 中的任务自动分配给 AI 编码代理（Codex），在隔离工作空间中完成编码工作。系统目标是将人工触发的编码流程转变为可重复的守护进程工作流，实现"策略即代码"的自动化研发流水线。

核心价值：
- 持续轮询 Issue Tracker，自动发现并分配待处理任务
- 每个 Issue 在独立工作空间中运行，互不干扰
- 工作流配置（`WORKFLOW.md`）随代码版本管理，团队可自定义 Agent 行为
- 结构化日志 + HTTP Dashboard，实时监控多个并发 Agent 运行状态
- Web 管理控制台提供可视化的项目、Issue、看板、告警管理能力

## 2. 三组件架构

系统由三个独立组件构成，各司其职：

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Symphony 系统                                  │
│                                                                       │
│  ┌─────────────────┐    REST API    ┌──────────────────────────────┐ │
│  │  web-frontend   │◄──────────────►│       web-platform           │ │
│  │  管理控制台      │                │       管理后台 API            │ │
│  │  React 18.3     │                │       Rust + Axum 0.8        │ │
│  │  MUI v6         │                │       SQLite + JWT            │ │
│  └─────────────────┘                └──────────────┬───────────────┘ │
│                                                     │                 │
│                                          进程管理 + stdio             │
│                                                     │                 │
│                                     ┌───────────────▼──────────────┐ │
│                                     │       rust-platform          │ │
│                                     │       编排运行时              │ │
│                                     │       Tokio 事件驱动          │ │
│                                     └──────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.1 rust-platform — 编排运行时

单二进制 CLI 应用，基于 Tokio 异步运行时构建，采用事件驱动的单线程状态机架构。负责：
- 轮询 Issue Tracker（Linear / GitHub / GitLab）
- 调度、启动、监控 Codex Agent 子进程
- 管理隔离工作空间目录
- 通过 HTTP API 暴露运行时状态

### 2.2 web-platform — 管理后台 API

Rust + Axum 0.8 构建的 RESTful API 服务，负责：
- 用户认证与授权（JWT + Argon2）
- 项目、Issue、看板、成员、告警的持久化管理（SQLite）
- rust-platform 子进程的生命周期管理（启动、停止、心跳监控）
- 外部集成（GitHub/GitLab 客户端、Azure OpenAI、DingTalk 通知）

### 2.3 web-frontend — 管理控制台

React 18.3 + TypeScript 5.5 构建的单页应用，负责：
- 项目管理、Issue 管理、看板视图
- 服务启停控制与状态监控
- 告警配置与并发控制
- AI 辅助 Issue 生成（流式渲染）

## 3. 组件交互关系

```
web-frontend
    │
    │  REST API（Bearer JWT）
    │  /api/* → Vite 代理 → :3000
    ▼
web-platform
    │                          │
    │  进程管理 + stdio          │  REST API
    │  spawn / kill / health    │  /api/v1/state
    ▼                          ▼
rust-platform ◄──────────────────────────────
    │                    HTTP /api/v1/*
    │
    ├── Tracker API（Linear GraphQL / GitHub REST / GitLab REST）
    │   轮询 Issue、查询状态
    │
    ├── Platform API（GitHub / GitLab）
    │   创建 PR、添加 Label、发表评论
    │
    └── Codex JSON-line stdio
        启动 Codex app-server 子进程
        发送 Turn 请求，接收事件流
```

交互说明：

| 交互路径 | 协议 | 方向 |
|----------|------|------|
| 前端 ↔ Web API | HTTP REST + JSON，Bearer JWT | 双向 |
| Web API → rust-platform | 子进程 spawn，stdin/stdout/stderr | Web API 主动管理 |
| Web API ← rust-platform | HTTP GET /api/v1/state（轮询） | Web API 主动查询 |
| rust-platform ↔ Tracker API | HTTPS REST / GraphQL | rust-platform 主动轮询 |
| rust-platform ↔ Platform API | HTTPS REST | rust-platform 主动调用 |
| rust-platform ↔ Codex | JSON-line stdio | rust-platform 主动管理 |

## 4. 部署拓扑

系统采用单机部署模式，所有组件运行在同一主机上：

```
主机
├── web-platform 进程（监听 :3000）
│   ├── 提供 REST API（/api/*）
│   ├── 提供 web-frontend 静态资源（生产模式）
│   └── 管理 rust-platform 子进程
│
├── rust-platform 子进程（由 web-platform 启动）
│   ├── 监听可选 HTTP 端口（Dashboard）
│   └── 管理 Codex 子进程
│
├── Codex app-server 子进程（由 rust-platform 启动，每 Issue 一个）
│
└── web-frontend 开发服务器（仅开发模式，Vite dev server）
    └── 代理 /api/* → :3000
```

生产部署：web-frontend 构建为静态资源，由 web-platform 的 Axum 静态文件服务提供。

开发模式：web-frontend 运行独立的 Vite dev server，通过 `vite.config.ts` 中的 proxy 配置将 `/api` 请求转发到 web-platform。

## 5. 技术栈总览

| 组件 | 类别 | 技术 | 版本 |
|------|------|------|------|
| rust-platform | 语言 | Rust | stable |
| rust-platform | 异步运行时 | Tokio | 1.x |
| rust-platform | HTTP 服务器 | Axum | 0.8 |
| rust-platform | HTTP 客户端 | Reqwest | 0.12 |
| rust-platform | 模板引擎 | Liquid | - |
| rust-platform | 配置格式 | YAML (serde_yaml) | - |
| rust-platform | 文件监听 | Notify | - |
| rust-platform | 原子配置 | Arc-swap | - |
| rust-platform | 日志 | Tracing | 0.1 |
| web-platform | 语言 | Rust | stable |
| web-platform | HTTP 框架 | Axum | 0.8 |
| web-platform | 数据库 | SQLite (rusqlite) | 0.32 |
| web-platform | 连接池 | r2d2 | 0.8 |
| web-platform | 数据库迁移 | Refinery | 0.8 |
| web-platform | 认证 | JWT (jsonwebtoken) | 9 |
| web-platform | 密码哈希 | Argon2 | 0.5 |
| web-platform | 加密存储 | AES-GCM | 0.10 |
| web-platform | API 文档 | utoipa + Swagger UI | 5/9 |
| web-frontend | 框架 | React | 18.3 |
| web-frontend | 语言 | TypeScript | 5.5 |
| web-frontend | UI 组件库 | MUI (Material UI) | v6 |
| web-frontend | CSS 框架 | Tailwind CSS | 3.4 |
| web-frontend | 构建工具 | Vite | 6.0 |
| web-frontend | 状态管理 | Zustand | 5.0 |
| web-frontend | 路由 | React Router | 6.26 |
| web-frontend | HTTP 客户端 | Axios | 1.7 |
| web-frontend | 单元测试 | Vitest + RTL | 2.x |
| web-frontend | E2E 测试 | Playwright | 1.49 |
| web-frontend | Mock | MSW | 2.x |

## 6. 与已有文档的关系

本文档是架构文档集的入口，提供系统级视图。更多细节请参阅：

- [`../architecture.md`](../architecture.md)：rust-platform 详细架构，包含完整的模块依赖图、并发模型、启动流程和关键设计决策
- [`../business.md`](../business.md)：业务流程文档，包含 Issue 生命周期、调度策略、容错机制和典型部署场景
- [`rust-platform.md`](rust-platform.md)：rust-platform 分层架构与核心模块详解
- [`web-platform.md`](web-platform.md)：web-platform 分层架构、认证授权、进程管理详解
- [`web-frontend.md`](web-frontend.md)：web-frontend 技术选型、路由设计、状态管理详解
- [`data-flow.md`](data-flow.md)：跨组件数据流详解
- [`decisions/`](decisions/)：架构决策记录（ADR）
