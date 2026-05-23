# Phase 1：基础框架

## 目标

搭建 Web 管理平台的基础框架，包含数据库、认证、用户管理、Graceful Shutdown 和 API 文档。

## 实现范围

### 后端 (web-platform crate)

| 模块 | 内容 |
|------|------|
| Workspace 改造 | 根 Cargo.toml 引入 workspace，新增 web-platform member |
| 数据库 | SQLite + refinery 迁移，建立全部表结构（users, user_configs, projects, project_members, system_configs, token_blacklist, alert_history） |
| Repository 抽象 | UserRepository + TokenBlacklistRepository trait，SqliteRepository 实现 |
| 认证 | JWT 单 Token 7天 HS256，Argon2id 密码哈希，内存黑名单 + SQLite 持久化，登录 rate limit |
| 用户管理 API | admin CRUD（创建/列表/删除/重置密码），个人配置（profile/config） |
| Graceful Shutdown | SIGINT/SIGTERM 信号处理，优雅关闭 HTTP server |
| OpenAPI 文档 | utoipa 注解 + Swagger UI (`/swagger-ui`) |
| 错误处理 | 统一 ResponseData 格式，错误码映射（AUTH_001→401, BIZ_001→400 等） |

### 前端 (web-frontend)

| 模块 | 内容 |
|------|------|
| 项目初始化 | Vite + React 18 + TypeScript |
| 路由 | react-router v6 |
| 组件库 | Google Material UI (@mui/material) |
| 状态管理 | Zustand |
| HTTP 客户端 | Axios + 拦截器（401 跳转登录） |
| 页面 | Login、用户管理（admin）、个人设置 |

### API 接口清单

```
POST   /api/auth/login                  登录
PUT    /api/auth/password               修改密码

GET    /api/user/profile                获取个人信息
PUT    /api/user/profile                更新个人信息
GET    /api/user/config                 获取个人配置（Token）
PUT    /api/user/config                 更新个人配置

GET    /api/admin/users                 用户列表（分页）
POST   /api/admin/users                 创建用户
DELETE /api/admin/users/:id             删除用户（软删除）
PUT    /api/admin/users/:id/reset-password  重置密码

GET    /health                          健康检查
GET    /swagger-ui                      API 文档
```

## 技术决策

| 决策项 | 选择 |
|--------|------|
| JWT Secret | 环境变量 `JWT_SECRET`，未设置拒绝启动 |
| SQLite 配置 | `PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;` |
| 连接池 | r2d2 + r2d2_sqlite |
| 密码哈希 | Argon2id, m=19456 KiB, t=2, p=1 |
| Token 加密 | AES-GCM 256-bit，密钥从 `ENCRYPTION_KEY` 环境变量读取 |
| Rate Limit | 登录：同一用户名 5次/分钟，同一 IP 20次/分钟 |
| 前端路由 | react-router v6 |
| 前端组件库 | Google Material UI |

## 目录结构

```
symphony-rust/
├── Cargo.toml                      # workspace 根配置（新增）
├── rust-platform/                  # 现有，不改动
├── web-platform/                   # 新增
│   ├── Cargo.toml
│   ├── migrations/
│   │   └── V001__init_schema.sql
│   └── src/
│       ├── main.rs
│       ├── config.rs
│       ├── db/
│       │   ├── mod.rs
│       │   └── migrations.rs
│       ├── repository/
│       │   ├── mod.rs
│       │   └── sqlite.rs
│       ├── models/
│       │   ├── mod.rs
│       │   ├── user.rs
│       │   └── response.rs
│       ├── auth/
│       │   ├── mod.rs
│       │   ├── jwt.rs
│       │   ├── password.rs
│       │   └── middleware.rs
│       ├── handlers/
│       │   ├── mod.rs
│       │   ├── auth.rs
│       │   ├── admin_users.rs
│       │   └── user_profile.rs
│       ├── error.rs
│       ├── router.rs
│       └── shutdown.rs
└── web-frontend/                   # 新增
    ├── package.json
    ├── vite.config.ts
    └── src/
        ├── main.tsx
        ├── App.tsx
        ├── router.tsx
        ├── api/
        ├── store/
        ├── pages/
        ├── components/
        ├── theme.ts
        └── types/
```

## 实现顺序

1. 根 Cargo.toml workspace 改造
2. web-platform crate 骨架（health check 能跑）
3. config.rs + 环境变量验证
4. db/ 模块 + 连接池 + refinery 迁移
5. models/ (User, UserConfig, ResponseData)
6. error.rs (WebPlatformError + IntoResponse)
7. repository/ trait + sqlite 实现
8. auth/ (password, JWT, blacklist)
9. middleware (jwt_auth, require_admin)
10. handlers/ (login, admin users, profile)
11. router.rs 组装
12. utoipa + Swagger UI
13. shutdown.rs
14. 集成测试
15. 前端项目初始化

## 验收标准

- [x] `cargo build --workspace` 编译通过
- [x] `cargo test -p web-platform` 测试通过
- [x] `curl /health` → 200
- [x] 登录获取 JWT → 用 JWT 访问 admin 接口 → 返回用户列表
- [x] `/swagger-ui` 可访问 OpenAPI 文档
- [x] 连续 6 次错误登录，第 6 次返回 429
- [x] 前端 Login 页面可登录，admin 页面可管理用户

## 环境变量

```env
DATABASE_URL=sqlite://./data/symphony-web.db
JWT_SECRET=<至少32字符的随机字符串>
ENCRYPTION_KEY=<64字符hex编码的256-bit密钥>
RUST_LOG=info
WEB_PLATFORM_PORT=3000
```

---

# 剩余里程碑概览

## Phase 2：项目管理

- Git URL 解析 + 平台识别（GitLab/GitHub）
- 项目 CRUD + WORKFLOW.md 配置管理
- 项目成员管理（手动添加 + 平台同步）
- Symphony 子进程生命周期管理（互斥锁 + PID 验证 + 启动前清理 + 崩溃恢复）
- 前端：项目列表、项目设置、成员管理页面

## Phase 3：看板

- GitLab/GitHub API 集成（Issue 列表、MR/PR 关联）
- 三列看板视图（待处理 / 处理中 / PR）
- Issue 创建（使用用户自己的 Token）
- AI 辅助 Issue 生成（Azure OpenAI gpt-5.5，SSE 流式 + Prompt 注入防护）
- 服务端内存缓存（singleflight 模式，5-10s TTL）
- 前端：看板页面、Issue 创建页面（含 AI 生成交互）

## Phase 4：协作与控制

- 多用户 Token 隔离（看板用当前用户 Token，服务用 owner Token）
- 全局并行控制（汇总各实例活跃 Agent 数，达上限暂停调度）
- 作者标识与筛选（Issue/PR 归属识别）
- 前端：作者筛选、并行数监控面板

## Phase 5：告警与通知

- 预警引擎（指标采集 + 规则评估：任务超时/失败/服务异常/并行饱和/连续失败/API 不可达）
- 通知分发器框架（NotificationChannel trait）
- 钉钉群机器人通知接入（Webhook + 签名）
- 告警历史与管理界面
- 前端：告警管理页面（历史 + 规则配置 + 通知渠道配置）
