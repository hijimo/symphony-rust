# 新开发者入门指南

## 环境准备

### 必要工具

**Rust 工具链（1.70+）**

通过 [rustup](https://rustup.rs) 安装：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup update stable
```

验证：

```bash
rustc --version   # rustc 1.70.0 或更高
cargo --version
```

**Node.js（18+）**

推荐通过 [nvm](https://github.com/nvm-sh/nvm) 管理：

```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh | bash
nvm install 18
nvm use 18
```

验证：

```bash
node --version   # v18.x.x 或更高
npm --version
```

**Git**

```bash
git --version   # 2.x 即可
```

---

## 仓库克隆

```bash
git clone <仓库地址>
cd symphony-rust
```

---

## 首次构建

**后端（Rust workspace）**

```bash
cargo build --workspace
```

这会同时编译 `web-platform`（管理平台）和 `symphony-platform`（rust-platform，被 web-platform 作为子进程拉起）。

**前端（React + Vite + TypeScript）**

```bash
cd web-frontend
npm install
cd ..
```

---

## 启动开发环境

### 一键启动（推荐）

```bash
./dev.sh
```

`dev.sh` 会自动：
1. 加载 `.env.local` 或 `.env`（如存在）
2. 设置开发用环境变量（JWT_SECRET、ENCRYPTION_KEY、DATABASE_URL 等）
3. 编译后端和 symphony-platform worker
4. 启动后端（`cargo run -p web-platform`，监听 `:3000`）
5. 启动前端（`npm run dev -- --port 5177`）

按 `Ctrl+C` 同时停止两个服务。

### 手动启动

如需分别控制，可在两个终端中分别运行：

**终端 1 — 后端**

```bash
export JWT_SECRET="dev-secret-key-at-least-32-chars-long"
export ENCRYPTION_KEY="MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY="
export DATABASE_URL="./data.db"
export SYMPHONY_BIN="./target/debug/symphony-platform"
export SYMPHONY_WORKSPACE_ROOT="./workspaces"
cargo run -p web-platform
```

**终端 2 — 前端**

```bash
cd web-frontend
npm run dev -- --port 5177
```

---

## 验证服务

| 服务 | 地址 | 说明 |
|------|------|------|
| 前端 | http://localhost:5177 | React 管理界面 |
| 后端 API | http://localhost:3000 | Axum HTTP 服务 |
| Swagger UI | http://localhost:3000/swagger-ui | 交互式 API 文档 |

打开 http://localhost:5177，使用默认凭据登录：

- 用户名：`admin`
- 密码：`admin123`

> 首次启动时，若数据库中不存在 admin 用户，系统会自动创建。密码由 `ADMIN_INIT_PASSWORD` 环境变量控制，开发环境默认为 `admin123`。

---

## 项目结构导览

```
symphony-rust/
├── web-platform/          # 管理平台后端（Rust + Axum）
│   └── src/
│       ├── handlers/      # HTTP 请求处理器
│       ├── services/      # 业务逻辑层
│       ├── repository/    # 数据访问层（SQLite）
│       ├── middleware/    # JWT 认证、权限检查中间件
│       ├── process_manager/ # rust-platform 子进程生命周期管理
│       ├── auth/          # 认证、密码哈希、限流
│       ├── crypto.rs      # AES-GCM 加密工具
│       └── config.rs      # 环境变量配置
│
├── rust-platform/         # Symphony 编排引擎（Rust + Tokio）
│   └── src/
│       ├── orchestrator/  # 事件循环、调度、协调、重试
│       ├── agent/         # AgentRunner、Codex 协议客户端
│       ├── tracker/       # Linear / GitLab Tracker 适配器
│       ├── platform/      # GitHub / GitLab Platform 适配器
│       ├── config/        # WORKFLOW.md 解析、热重载
│       ├── workspace/     # 工作空间目录生命周期
│       └── prompt/        # Liquid 模板引擎
│
├── web-frontend/          # 管理界面前端（React + Vite + TypeScript）
│   └── src/
│       ├── pages/         # 页面组件
│       ├── components/    # 通用 UI 组件
│       ├── api/           # API 请求函数
│       └── store/         # 状态管理
│
├── docs/                  # 项目文档
│   ├── business.md        # 业务背景与产品定义
│   ├── architecture.md    # 系统架构（rust-platform）
│   └── technical/         # 本目录：技术实现文档
│
├── WORKFLOW.md            # 当前项目的工作流配置示例
├── WORKFLOW.md.github     # GitHub 平台配置示例
├── WORKFLOW.md.Linear.md  # Linear 平台配置示例
├── dev.sh                 # 一键开发环境启动脚本
└── Cargo.toml             # Rust workspace 配置
```

---

## 推荐阅读顺序

1. **`docs/business.md`** — 了解产品背景、核心概念和用户场景，建立业务认知
2. **`docs/architecture.md`** — 深入理解 rust-platform 的架构设计、事件循环和并发模型
3. **`docs/technical/getting-started.md`**（本文）— 搭建开发环境
4. **`docs/technical/configuration.md`** — 理解 WORKFLOW.md 配置格式和所有可用字段
5. **`docs/technical/development-guide.md`** — 日常开发工作流、调试技巧
6. **`docs/technical/modules/`** — 按需深入各模块实现细节

---

## 常见问题

**cargo build 失败，提示缺少系统依赖**

macOS 需要 Xcode Command Line Tools：

```bash
xcode-select --install
```

**npm install 失败**

确认 Node.js 版本 >= 18，并尝试清理缓存：

```bash
npm cache clean --force
npm install
```

**后端启动报 `JWT_SECRET must be at least 32 characters`**

确保 `JWT_SECRET` 环境变量长度不少于 32 个字符。使用 `dev.sh` 启动时已自动设置。
