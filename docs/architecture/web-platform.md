# web-platform 架构文档

## 1. 概述

web-platform 是 Symphony 的管理后台 API 服务，基于 Rust + Axum 0.8 构建，提供用户认证、项目管理、Issue 管理、看板、告警等 RESTful API，并负责 rust-platform 子进程的完整生命周期管理。

## 2. 分层架构

```
┌─────────────────────────────────────────────────────┐
│  Router 层                                           │
│  router.rs（路由注册、中间件链挂载）                  │
└────────────────────────┬────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────┐
│  Handler 层                                          │
│  handlers/（请求解析、参数校验、响应序列化）           │
└────────────────────────┬────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────┐
│  Service 层                                          │
│  services/（业务逻辑、跨 Repository 协调）            │
└────────────────────────┬────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────┐
│  Repository 层                                       │
│  repository/（数据访问，SQL 查询封装）                │
└────────────────────────┬────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────┐
│  DB 层                                               │
│  db/（SQLite 连接池，Refinery 迁移）                  │
└─────────────────────────────────────────────────────┘
```

## 3. Axum 中间件链

请求按以下顺序经过中间件处理：

```
请求
  │
  ▼
CORS（tower-http CorsLayer）
  │  允许跨域，开发模式下允许所有来源
  ▼
JWT Auth（auth/middleware.rs）
  │  验证 Authorization: Bearer <token>
  │  公开路由（/api/auth/*、/health）跳过
  │  admin 路由额外检查 role = admin
  ▼
Handler
  │  在 Handler 内部按需调用：
  │  - require_project_member()：验证项目访问权限
  │  - RateLimiter：防止接口滥用（如登录接口）
  ▼
Response
```

> 注：Project Access 和 Rate Limit 不是 Axum middleware layer，而是在 Handler 内部按需调用的逻辑。

### 路由分组

| 分组 | 路径前缀 | 认证要求 |
|------|----------|----------|
| public | `/api/auth/*` | 无需认证 |
| user | `/api/user/*`、`/api/projects/*` | JWT 认证 |
| admin | `/api/admin/*` | JWT 认证 + admin 角色 |
| project | `/api/projects/{id}/*` | JWT 认证 + Handler 内检查项目访问权限 |

## 4. 认证与授权

### JWT 认证

- 登录成功后签发 JWT，包含 `user_id`、`role`、`exp` 等 Claims
- 使用 `jsonwebtoken` crate，HS256 算法，密钥从环境变量读取
- Token 过期后前端自动登出（Axios 响应拦截器检测 401）

### 密码安全

- 密码使用 `argon2` 哈希存储，不可逆
- 注册/修改密码时自动生成随机 salt

### 角色模型

| 角色 | 权限范围 |
|------|----------|
| `admin` | 全部接口，包括用户管理、系统配置、并发控制 |
| `user` | 项目管理、Issue、看板、个人设置 |

## 5. 数据存储

### SQLite + r2d2 连接池

- 使用 `rusqlite`（bundled feature，无需系统安装 SQLite）
- `r2d2` + `r2d2_sqlite` 提供连接池，支持并发读写
- 数据库文件路径通过环境变量配置

### Refinery 迁移

- 数据库 Schema 通过 `refinery` crate 管理版本化迁移
- 迁移文件位于 `db/` 目录，按版本号顺序执行
- 服务启动时自动执行待执行的迁移

### AES-GCM 加密存储

- 敏感数据（API Token、密钥等）使用 `aes-gcm` 加密后存储
- 加密密钥从环境变量读取，不持久化到数据库
- `crypto.rs` 提供统一的加解密工具函数

## 6. 进程管理器

进程管理器（`process_manager/`）负责 rust-platform 子进程的完整生命周期，是 web-platform 的核心能力之一。

### 模块组成

| 模块 | 职责 |
|------|------|
| `spawn.rs` | 启动 rust-platform 子进程，传递配置参数和环境变量 |
| `watcher.rs` | 心跳监控，定期检查子进程健康状态 |
| `cleanup.rs` | 孤儿进程清理，处理异常退出遗留的进程 |
| `pid_verify.rs` | PID 验证，确认进程身份与预期一致 |

### Lease 机制

为解决多实例竞争和孤儿进程问题，进程管理器采用三层 Lease 标识：

```
web_instance_id     — web-platform 实例唯一 ID（启动时生成）
    │
    └── lifecycle_op_id  — 每次启动/停止操作的唯一 ID
            │
            └── service_generation  — 服务代次，每次重启递增
```

工作原理：
1. 启动子进程时，将三层 ID 写入数据库和子进程环境变量
2. 子进程启动后将自身 PID 和 Lease 信息注册回数据库
3. 心跳监控定期验证 PID 和 Lease 是否匹配
4. 若检测到 Lease 不匹配（如旧实例遗留进程），触发孤儿清理

### 子进程环境变量

`spawn.rs` 启动子进程时继承代理相关环境变量（`http_proxy`、`https_proxy`、`all_proxy` 及大写形式），确保子进程能通过代理访问外部 API。

> 注意：子进程 stderr 必须被消费（drain）或设为 `Stdio::null()`，否则 stderr pipe buffer 满后会导致子进程死锁。详见 `CLAUDE.md`。

## 7. Handler 模块列表

| 模块 | 路径 | 主要功能 |
|------|------|----------|
| `auth.rs` | `/api/auth/*` | 登录、密码修改 |
| `admin_users.rs` | `/api/admin/users/*` | 用户 CRUD、密码重置 |
| `admin_config.rs` | `/api/admin/config` | 系统配置管理 |
| `projects.rs` | `/api/projects` | 项目 CRUD |
| `project_service.rs` | `/api/projects/{id}/start\|stop\|restart\|status\|diagnostics` | 服务启停、状态查询 |
| `project_workflow.rs` | `/api/projects/{id}/workflow` | WORKFLOW.md 读写、重置 |
| `issues.rs` | `/api/projects/{id}/issues/*` | Issue CRUD |
| `kanban.rs` | `/api/projects/{id}/kanban` | 看板视图 |
| `alerts.rs` | `/api/admin/alerts/*` | 告警规则与通道配置 |
| `concurrency.rs` | `/api/admin/concurrency/*` | 并发控制配置、SSE 事件流 |
| `project_members.rs` | `/api/projects/{id}/members/*` | 项目成员管理 |
| `merge_requests.rs` | `/api/projects/{id}/mrs/*` | MR 列表与创建 |
| `issue_mrs.rs` | `/api/projects/{id}/issues/{iid}/mrs` | Issue 关联 MR |
| `contributors.rs` | `/api/projects/{id}/contributors` | 贡献者统计 |
| `ai_generate.rs` | `/api/projects/{id}/issues/ai-generate` | AI 辅助 Issue 生成（SSE 流式） |
| `network_proxy.rs` | `/api/admin/network-proxy/*` | 网络代理配置 |
| `user_profile.rs` | `/api/user/*` | 个人资料、配置管理 |
| `token_validation.rs` | `/api/user/config/validate-token` | 平台 Token 验证 |

## 8. 外部集成

### Git 平台客户端

| 平台 | 功能 |
|------|------|
| GitHub | Issue 查询、Label 管理、PR 创建、凭证验证 |
| GitLab | Issue 查询、Label 管理、MR 创建、凭证验证 |

客户端通过 `reqwest` 调用各平台 REST API，支持通过系统代理配置访问。

### AI 服务

- 集成 Azure OpenAI，用于 AI 辅助 Issue 生成
- 支持 SSE（Server-Sent Events）流式响应，前端实时渲染生成内容
- Handler：`ai_generate.rs`，API：`/api/ai/generate`

### 通知服务

- 集成 DingTalk（钉钉）Webhook，用于告警通知
- `notification/` 模块封装通知发送逻辑
- 告警触发条件由 `alert/` 模块的告警引擎评估

## 9. API 文档

使用 `utoipa` + `utoipa-swagger-ui` 自动生成 OpenAPI 文档，访问路径：`/swagger-ui/`。
