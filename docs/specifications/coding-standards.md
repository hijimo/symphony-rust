# 编码规范

## Rust 编码规范

### 命名规范

- **变量、函数、模块**：`snake_case`
- **类型、Trait、枚举**：`CamelCase`
- **常量、静态变量**：`UPPER_SNAKE_CASE`
- **生命周期参数**：`'a`、`'b` 等简短小写名称
- **泛型参数**：单字母大写（`T`、`E`）或描述性 CamelCase（`TItem`）

### 错误处理

使用 `thiserror` 定义错误类型，使用 `Result<T>` 传播错误，禁止 `unwrap()`（测试代码除外）。

```rust
// 正确：定义领域错误类型
#[derive(Debug, thiserror::Error)]
pub enum MyError {
    #[error("resource not found: {0}")]
    NotFound(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// 正确：使用 ? 传播错误
pub fn do_something() -> Result<String, MyError> {
    let content = std::fs::read_to_string("file.txt")?;
    Ok(content)
}
```

错误消息不要硬编码平台名称（如 "GitLab API"），因为同一错误变体可能被多个平台共用，使用通用名称（如 "Tracker API"）。

### async/await 模式

- 所有异步函数使用 `async fn` + `await`，不使用手动 `Future`
- 使用 `tokio::spawn` 启动后台任务，保存 `JoinHandle` 以便等待或取消
- 使用 `tokio_util::sync::CancellationToken` 实现协作式取消
- 避免在 async 上下文中调用阻塞操作，使用 `tokio::task::spawn_blocking`

### Trait 设计

- 为可替换的外部依赖（Tracker、Platform）定义 Trait，便于测试时 mock
- Trait 方法使用 `async fn`（需要 `async_trait` 或 RPITIT）
- 保持 Trait 接口最小化，单一职责

### 代码质量工具

- **Clippy**：`cargo clippy -- -D warnings`，所有 warning 必须修复，不允许 `#[allow(clippy::...)]` 绕过（除非有充分理由并添加注释）
- **格式化**：`cargo fmt`，CI 中执行 `cargo fmt --check`
- **测试**：`cargo test`，新功能必须有对应测试

### 模块组织

- 每个模块通过 `mod.rs` 或同名文件导出公共接口
- 内部实现细节标记为 `pub(crate)` 或私有
- 避免循环依赖，依赖方向：`handlers` → `services` → `repository` → `models`

### 日志规范

使用 `tracing` crate，不使用 `println!`：

```rust
// 结构化日志，使用 span 和 event
tracing::info!(issue_id = %id, "starting worker");
tracing::warn!(pid = ?pid, "process did not stop gracefully");
tracing::error!(error = %e, "failed to connect");
```

- 关键操作使用 `tracing::info!`
- 预期内的异常使用 `tracing::warn!`
- 不可恢复的错误使用 `tracing::error!`
- 调试信息使用 `tracing::debug!`

---

## 已知陷阱与必须遵守的规则

### 状态字符串归一化必须全局一致

项目中存在多处 state 归一化逻辑，**必须**使用相同的规则：`trim + lowercase + 空格/连字符转下划线`。

相关实现位置：
- `rust-platform/src/models/mod.rs` → `normalize_state()`
- `rust-platform/src/tracker/gitlab.rs` → `normalize_tracker_state()`
- `rust-platform/src/main.rs` → `build_workflow_states()`

> ⚠️ **已知不一致**：`build_workflow_states()` 当前仅做 `.to_lowercase().replace(' ', "_")`（无 trim、不处理连字符），与 `normalize_state()` 的完整规则（trim + lowercase + 空格/连字符转下划线）存在差异。新增 state 比较逻辑时应使用 `normalize_state()` 而非 `build_workflow_states()` 的逻辑。

**背景**：`GitlabTrackerAdapter` 返回的 `TrackerIssue.state` 是 state_key 形式（如 `"in_progress"`），而 `ServiceConfig.active_states` 保存的是原始形式（如 `"In Progress"`）。如果 reconciler 的 `normalize_state` 不处理空格→下划线转换，就会误判 issue 不在 active 状态而终止正在运行的 worker。

**规则**：新增任何 state 比较逻辑时，必须使用 `normalize_state()` 并确保它与 `normalize_tracker_state()` 行为一致。

```rust
// 正确：使用 normalize_state 比较
let normalized = normalize_state(&issue.state);
if config.active_states.iter().any(|s| normalize_state(s) == normalized) {
    // ...
}

// 错误：直接字符串比较
if config.active_states.contains(&issue.state) {
    // ...
}
```

---

### 子进程 stderr 必须被消费

启动子进程时如果 stderr 设为 `Stdio::piped()`，**必须**在后台 drain stderr。否则当子进程 stderr 输出超过 pipe buffer（64KB）时，子进程会被阻塞在 `write(stderr)`，导致 stdout 也无法响应，造成 deadlock 超时。

```rust
// 正确：后台 drain stderr
if let Some(stderr) = child.stderr.take() {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => break,
                Ok(_) => tracing::debug!("stderr: {}", line.trim_end()),
            }
        }
    });
}

// 如果不需要 stderr 内容，使用 null
command.stderr(Stdio::null());
```

---

### 错误消息不要硬编码平台名称

`TrackerError` 等共用错误类型的 `#[error(...)]` 消息不要写死特定平台名（如 "Linear API"、"GitLab API"），因为同一个错误变体会被 GitHub/GitLab/Linear 三种 tracker 共用。

```rust
// 错误：硬编码平台名
#[error("Linear API request failed: {0}")]
ApiError(String),

// 正确：使用通用名称
#[error("Tracker API request failed: {0}")]
ApiError(String),
```

---

### 子进程环境变量传递

`web-platform/src/process_manager/spawn.rs` 启动 symphony 子进程时，需要继承代理相关环境变量（`http_proxy`、`https_proxy`、`all_proxy` 及大写形式），否则子进程无法通过代理访问外部 API。

同样，`CodexClient` 启动 Codex 子进程时，通过 `proxy_command("bash")` 确保代理环境变量被正确传递。

---

## TypeScript/React 规范

### 组件规范

- 使用函数组件 + Hooks，不使用 class 组件
- 组件文件名使用 PascalCase（`UserProfile.tsx`）
- 每个文件只导出一个主组件

### 类型定义

- 所有类型定义放在 `types/` 目录或与组件同名的 `.types.ts` 文件
- 使用 `interface` 定义对象类型，使用 `type` 定义联合类型和工具类型
- API 响应类型与后端 `ResponseData<T>` 结构对应

### 状态管理

- 全局状态使用 Zustand store，store 文件放在 `store/` 目录
- 组件本地状态使用 `useState`/`useReducer`
- 服务端状态（API 数据）使用 React Query 或 SWR

### 代码质量

- **ESLint**：遵循项目 ESLint 配置，CI 中执行 `npm run lint`
- **TypeScript**：严格模式，不使用 `any`（除非有充分理由）
- **测试**：`npm test`，组件测试使用 React Testing Library

---

## 通用规范

### 注释规范

- 注释最少化：代码应自解释，只在必要时添加注释
- 公共 API（pub fn、pub struct）必须有文档注释（`///`）
- 复杂算法或非直觉的实现添加解释性注释
- 不要注释掉代码，直接删除

### 环境变量

- 环境变量名使用 `UPPER_SNAKE_CASE`
- 敏感配置（token、密钥）通过环境变量传入，不硬编码
- 关键环境变量：`ENCRYPTION_KEY`（AES 加密密钥）、`JWT_SECRET`（JWT 签名密钥）、`DATABASE_URL`（数据库路径）

### 前端设计规范

前端界面开发必须严格遵循 `design/design.md` 设计规范（Architectural Logic 设计系统），包括色彩体系、排版规范、间距与布局、组件样式、圆角策略。详见 `design/design.md`。
