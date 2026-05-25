# 跨组件数据流

## 1. 概述

本文档描述 Symphony 三个组件（web-frontend、web-platform、rust-platform）之间的主要数据流路径，以及各组件与外部系统的交互流程。

## 2. 用户操作流

用户通过管理控制台执行操作的通用数据流：

```
用户操作（点击、表单提交）
    │
    ▼
web-frontend（React 组件）
    │ Zustand action 触发
    ▼
api/ 层（Axios）
    │ camelCase → snake_case
    │ 注入 Bearer token
    │ HTTP POST/PUT/DELETE /api/*
    ▼
web-platform（Axum Handler）
    │ JWT 验证 → 权限检查
    ▼
Service 层（业务逻辑）
    │
    ▼
Repository 层（SQL 查询）
    │
    ▼
SQLite 数据库
    │
    ▼（响应路径）
Repository → Service → Handler
    │ snake_case JSON 响应
    ▼
api/ 层（Axios 响应拦截器）
    │ snake_case → camelCase
    ▼
Zustand Store 更新
    │
    ▼
React 组件重渲染
```

## 3. 服务启动流

用户在控制台点击"启动服务"后的完整流程：

```
用户点击"启动服务"
    │
    ▼
web-frontend → POST /api/projects/{id}/start
    │
    ▼
web-platform Handler（project_service.rs）
    │ 生成 lifecycle_op_id、service_generation
    │ 写入数据库（pending 状态）
    ▼
ProcessManager（process_manager/spawn.rs）
    │ 构建启动命令（rust-platform 二进制路径 + 参数）
    │ 继承代理环境变量（http_proxy 等）
    │ spawn 子进程，stderr drain 或 null
    ▼
rust-platform 子进程启动
    │
    ├── 解析 CLI 参数
    ├── 加载 WORKFLOW.md
    ├── 构建 ServiceConfig
    ├── 初始化 Tracker Client
    ├── 启动 HTTP Server（可选端口）
    └── 进入 Orchestrator 事件循环
    │
    ▼
子进程将 PID + Lease 信息注册回 web-platform
    │
    ▼
web-platform 更新数据库（running 状态）
    │
    ▼
web-frontend 轮询状态 → 显示"运行中"
```

## 4. Issue 处理流

从 Issue 被发现到编码完成的完整流程：

```
Tracker API（Linear / GitHub / GitLab）
    │ rust-platform 定时轮询（Tick 事件）
    ▼
Orchestrator → Scheduler
    │ 过滤已 claimed/completed
    │ 检查并发上限
    │ 按优先级排序
    ▼
spawn_worker(issue)
    │
    ├── WorkspaceManager：创建/复用工作空间目录
    ├── 执行 before_run Hook（如 git pull）
    ├── 启动 Codex app-server 子进程
    │       │ bash -lc "{codex_command}"
    │       │ 工作目录 = workspace/{issue-id}/
    │
    └── 多轮循环：
            ├── 渲染 Prompt（Liquid 模板）
            ├── 发送 Turn 请求（JSON-line stdin）
            ├── 流式接收事件（Codex stdout）
            │       └── CodexUpdate → Orchestrator（token 统计）
            ├── 检查 Issue 状态是否仍活跃
            └── 检查轮次上限
    │
    ▼
Codex 在 workspace 中执行编码
    │ 修改文件、运行测试
    ▼
Codex 通过 Platform API 工具提交结果
    │ create_pull_request（GitHub / GitLab）
    │ add_label、create_comment
    ▼
Worker 正常退出 → WorkerExitNormal 事件
    │
    ▼
Orchestrator：续行检查 Issue 状态
    │ 若 Issue 已进入终态 → 释放资源
    │ 若 Issue 仍活跃 → 短延迟后续行
    ▼
WorkspaceManager：GC 清理已完成工作空间
```

## 5. 状态查询流

前端轮询服务运行状态的数据流：

```
web-frontend（定时轮询，如每 5 秒）
    │ GET /api/projects/{id}/status
    ▼
web-platform Handler（project_service.rs）
    │ 查询数据库获取 PID 和 Lease 信息
    ▼
ProcessManager（pid_verify.rs）
    │ 验证 PID 是否存活（kill -0）
    │ 验证 Lease 是否匹配
    ▼
若进程存活：HTTP GET rust-platform /api/v1/state
    │ 通过 oneshot channel 查询 Orchestrator
    ▼
Orchestrator 构建 StateResponse 快照
    │ running workers、claimed set、retry queue
    │ token 统计、并发使用率
    ▼
rust-platform HTTP 响应 → web-platform
    │
    ▼
web-platform 聚合状态 → JSON 响应
    │
    ▼
web-frontend 更新 projectStore
    │
    ▼
React 组件渲染最新状态（运行中 Agent 数、token 消耗等）
```

## 6. AI 生成流

用户使用 AI 辅助生成 Issue 内容的流程：

```
用户点击"AI 生成"，输入描述
    │
    ▼
web-frontend（issueStore.generateWithAI()）
    │ 设置 generating = true
    │ 清空 streamContent
    │ fetch POST /api/ai/generate（SSE）
    ▼
web-platform Handler（ai_generate.rs）
    │ 读取 Azure OpenAI 配置（加密存储）
    │ 构建 OpenAI Chat Completion 请求
    ▼
Azure OpenAI API（流式响应）
    │ SSE stream，逐 token 返回
    ▼
web-platform 透传 SSE 流
    │ Content-Type: text/event-stream
    ▼
web-frontend Axios / fetch 接收 SSE
    │ 逐行解析 data: {...}
    │ 追加 token 到 issueStore.streamContent
    ▼
React 组件实时渲染 streamContent
    │
    ▼
流结束 → generating = false → 用户确认内容
```

## 7. 告警流

rust-platform 运行指标触发告警通知的流程：

```
rust-platform 运行时指标
    │ token 消耗、错误率、Worker 状态
    ▼
web-platform 告警引擎（alert/）
    │ 定期从 rust-platform /api/v1/state 拉取指标
    │ 与告警规则（数据库存储）对比
    │ 评估触发条件（阈值、时间窗口）
    ▼
触发告警
    │
    ├── 写入告警历史（SQLite）
    │
    └── 通知服务（notification/）
            │ 构建 DingTalk Webhook 消息
            ▼
        DingTalk 群机器人
            │ 推送告警消息到指定群
            ▼
        运维人员收到通知
    │
    ▼
web-frontend 告警列表页面
    │ 轮询 GET /api/admin/alerts/history
    ▼
展示告警历史和当前状态
```

## 8. 配置变更流

用户修改 WORKFLOW.md 后配置热重载的流程：

```
用户在控制台编辑 WORKFLOW.md
    │ PUT /api/projects/:id/workflow
    ▼
web-platform 写入文件系统
    │ 将新内容写入 rust-platform 工作目录下的 WORKFLOW.md
    ▼
rust-platform（notify crate 文件监听）
    │ 检测到文件变更事件（inotify / FSEvents / ReadDirectoryChanges）
    ▼
config/watcher.rs
    │ 读取新文件内容
    │ workflow_loader 解析 YAML Front Matter + Markdown 正文
    │ validator 校验配置完整性
    ▼
校验通过
    │
    ▼
ConfigHolder（ArcSwap 原子替换）
    │ arc_swap::ArcSwap::store(new_config)
    │ 无锁，所有读者下次 load() 获取新版本
    ▼
发送 ConfigReloaded 事件 → Orchestrator mpsc channel
    │
    ▼
Orchestrator 下次 Tick 使用新配置
    │ 新的轮询间隔、并发上限、重试策略立即生效
    │ 新的 Prompt 模板在下次 Worker 启动时生效
    ▼
无需重启服务，配置变更透明生效
```
