# 开发指南

## 构建命令

### Rust 后端

```bash
# 完整构建（所有 crate）
cargo build --workspace

# 仅检查语法和类型，不生成二进制（速度更快）
cargo check --workspace

# Clippy 静态分析（CI 要求通过）
cargo clippy --workspace -- -D warnings

# Release 构建
cargo build --release --workspace
```

### 前端

```bash
cd web-frontend

# 开发模式（Vite HMR）
npm run dev -- --port 5177

# 生产构建
npm run build

# 代码检查
npm run lint

# 类型检查
npx tsc --noEmit
```

---

## 运行测试

### Rust 单元测试 / 集成测试

```bash
# 运行所有测试
cargo test --workspace

# 运行特定 crate 的测试
cargo test -p web-platform
cargo test -p symphony-platform

# 运行特定测试（按名称过滤）
cargo test orchestrator

# 显示测试输出（不捕获 stdout）
cargo test -- --nocapture

# 并行度控制（避免端口冲突）
cargo test -- --test-threads=1
```

### 前端测试

```bash
cd web-frontend

# 单元测试
npm test

# 端到端测试
npm run test:e2e
```

---

## 调试技巧

### RUST_LOG 环境变量

Symphony 使用 `tracing` + `tracing-subscriber` 输出结构化日志。通过 `RUST_LOG` 控制日志级别：

```bash
# 开发环境推荐（详细日志）
export RUST_LOG="web_platform=debug,tower_http=debug"

# 查看 orchestrator 详细日志
export RUST_LOG="symphony_platform=debug"

# 只看 warn 及以上
export RUST_LOG="warn"

# 特定模块 trace 级别
export RUST_LOG="symphony_platform::orchestrator=trace,symphony_platform::tracker=debug"
```

日志级别从低到高：`trace` < `debug` < `info` < `warn` < `error`

### 关键 span 说明

| span / 字段 | 含义 |
|-------------|------|
| `issue_id` | Tracker 内部 ID |
| `identifier` | 人类可读标识（如 `ABC-123`） |
| `attempt` | 重试次数 |
| `pid` | 子进程 PID |
| `project_id` | web-platform 项目 ID |

### 浏览器 DevTools

前端使用 React + Vite，调试建议：

- Network 面板：查看 API 请求/响应，SSE 流式事件
- Console：React 错误边界会打印详细堆栈
- React DevTools 扩展：检查组件状态和 props

### 查看运行时状态

rust-platform 暴露状态查询 API（需在 WORKFLOW.md 中配置 `server.port`）：

```bash
curl http://localhost:<port>/api/v1/state | jq .
```

web-platform 的项目状态通过管理界面或 API 查看：

```bash
curl -H "Authorization: Bearer <token>" http://localhost:3000/api/v1/projects
```

---

## 热重载

### 前端

Vite 提供 HMR（Hot Module Replacement），保存文件后浏览器自动刷新，无需手动操作。

### 后端（web-platform）

Rust 不支持运行时热重载。修改后端代码后需重新编译并重启：

```bash
# 停止当前进程（Ctrl+C），然后重新运行
cargo run -p web-platform
```

或使用 `cargo-watch` 自动重启：

```bash
cargo install cargo-watch
cargo watch -x "run -p web-platform"
```

### WORKFLOW.md 热重载

rust-platform 使用 `notify` crate 监听 `WORKFLOW.md` 文件变更。修改配置文件后**无需重启 rust-platform**，新配置会通过 `arc-swap` 原子替换生效。

注意：
- 已在运行的 worker 使用启动时的配置快照，不受热重载影响
- 新调度的 worker 使用最新配置
- 若新配置解析失败，保留上次有效配置，并输出 warn 日志

---

## 数据库操作

### 自动迁移

web-platform 使用 [Refinery](https://github.com/rust-db/refinery) 管理数据库迁移。每次启动时自动检测并执行未应用的迁移脚本，无需手动操作。

迁移文件位于：

```
web-platform/src/db/migrations/
```

### SQLite 文件位置

默认路径由 `DATABASE_URL` 环境变量控制，开发环境默认为项目根目录的 `data.db`：

```bash
ls -la data.db data.db-shm data.db-wal
```

### 手动查看数据库

```bash
# 安装 sqlite3 CLI（macOS）
brew install sqlite

# 打开数据库
sqlite3 data.db

# 常用命令
.tables                          -- 列出所有表
.schema projects                 -- 查看表结构
SELECT * FROM projects LIMIT 10; -- 查询数据
.quit
```

---

## 添加新 API 端点

以添加 `GET /api/v1/projects/:id/logs` 为例：

1. **Handler** — 在 `web-platform/src/handlers/` 下新建或修改文件，实现请求处理函数：

```rust
pub async fn get_project_logs(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    auth: AuthUser,
) -> Result<Json<LogsResponse>, WebPlatformError> {
    // 实现逻辑
}
```

2. **Service** — 在 `web-platform/src/services/` 中实现业务逻辑（如需要）

3. **Repository** — 在 `web-platform/src/repository/` 中添加数据访问方法（如需要）

4. **Router 注册** — 在 `web-platform/src/router.rs` 中注册路由：

```rust
.route("/api/v1/projects/:id/logs", get(handlers::project::get_project_logs))
```

5. **OpenAPI 注解** — 在 handler 函数上添加 `utoipa` 注解，使其出现在 Swagger UI：

```rust
#[utoipa::path(
    get,
    path = "/api/v1/projects/{id}/logs",
    responses((status = 200, description = "项目日志", body = LogsResponse)),
    security(("bearer_auth" = []))
)]
```

---

## 添加新前端页面

以添加"项目日志"页面为例：

1. **Page 组件** — 在 `web-frontend/src/pages/` 下新建 `ProjectLogs.tsx`

2. **Router 注册** — 在路由配置文件中添加路由：

```tsx
{ path: "/projects/:id/logs", element: <ProjectLogs /> }
```

3. **API 函数** — 在 `web-frontend/src/api/` 中添加请求函数：

```ts
export async function getProjectLogs(projectId: number): Promise<LogsResponse> {
  return apiFetch(`/api/v1/projects/${projectId}/logs`);
}
```

4. **Store（如需要）** — 若页面需要全局状态，在 `web-frontend/src/store/` 中添加对应 slice

---

## 常用开发脚本

```bash
# 一键启动开发环境
./dev.sh

# 仅构建（不运行）
cargo build --workspace

# 格式化代码
cargo fmt --all
cd web-frontend && npx prettier --write src/

# 检查未使用的依赖
cargo +nightly udeps --workspace

# 查看编译产物大小
ls -lh target/debug/web-platform target/debug/symphony-platform

# 清理构建缓存
cargo clean

# 查看后端日志（dev.sh 运行时）
# 日志直接输出到终端，rust-platform 子进程日志写入工作空间目录下的 symphony.log
cat <SYMPHONY_WORKSPACE_ROOT>/<project_id>/symphony.log
```
