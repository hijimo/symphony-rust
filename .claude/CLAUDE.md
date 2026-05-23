# Project Guidelines

## Frontend Design

前端界面开发必须严格遵循 `design/design.md` 设计规范（Architectural Logic 设计系统）。包括：
- 色彩体系（tonal layering、surface hierarchy）
- 排版规范（Inter 字体、type scale）
- 间距与布局（4px base unit、256px sidebar、12-column grid）
- 组件样式（渐变按钮、filled 输入框、无阴影静态卡片）
- 圆角策略（4px 按钮/输入框、8px 卡片/容器）

## 已知陷阱与编码规范

### 状态字符串归一化必须全局一致

项目中存在多处 state 归一化逻辑，它们**必须**使用相同的规则（trim + lowercase + 空格/连字符转下划线）：
- `rust-platform/src/models/mod.rs` → `normalize_state()`
- `rust-platform/src/tracker/gitlab.rs` → `normalize_tracker_state()`
- `rust-platform/src/main.rs` → `build_workflow_states()`

**教训**：`GitlabTrackerAdapter` 返回的 `TrackerIssue.state` 是 state_key 形式（如 `"in_progress"`），而 `ServiceConfig.active_states` 保存的是原始形式（如 `"In Progress"`）。如果 reconciler 的 `normalize_state` 不处理空格→下划线转换，就会误判 issue 不在 active 状态而终止正在运行的 worker。

**规则**：新增任何 state 比较逻辑时，必须使用 `normalize_state()` 并确保它与 `normalize_tracker_state()` 行为一致。

### 子进程 stderr 必须被消费

启动子进程时如果 stderr 设为 `Stdio::piped()`，**必须**在后台 drain stderr。否则当子进程 stderr 输出超过 pipe buffer（64KB）时，子进程会被阻塞在 write(stderr)，导致 stdout 也无法响应，造成 deadlock 超时。

**规则**：凡是 `Stdio::piped()` 的 stderr，必须 spawn 后台任务读取。如果不需要 stderr 内容，用 `Stdio::null()` 代替。

### 错误消息不要硬编码平台名称

`TrackerError` 等共用错误类型的 `#[error(...)]` 消息不要写死特定平台名（如 "Linear API"），因为同一个错误变体会被 GitHub/GitLab/Linear 三种 tracker 共用。使用通用名称（如 "Tracker API"）。

### 子进程环境变量传递

`web-platform/src/process_manager/spawn.rs` 启动 symphony 子进程时，需要继承代理相关环境变量（`http_proxy`、`https_proxy`、`all_proxy` 及大写形式），否则子进程无法通过代理访问外部 API。

### 测试编写要求

编写 reconciler / orchestrator 相关测试时：
- **不要**在测试中直接构造理想化的 state 字符串（如直接传 `"In Progress"`）。应该模拟真实数据流路径：从 platform adapter 返回 state_key，再经过 reconciler 判断。
- 加入交叉匹配用例：`active_states` 用原始形式，`refreshed_states` 用 state_key 形式，验证归一化后能正确匹配。
- 子进程相关测试需要考虑 stdio pipe buffer 的影响，不能假设 stderr 永远为空。
