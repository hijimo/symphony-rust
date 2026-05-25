# 配置参考

## WORKFLOW.md 格式

`WORKFLOW.md` 是 rust-platform 的单一配置源，采用 YAML front matter + Liquid 模板正文的格式：

```
---
<YAML 配置>
---

<Liquid 模板正文（Prompt）>
```

以下是所有支持的配置字段说明。

---

### tracker 块

配置 Issue 追踪器。

```yaml
tracker:
  kind: gitlab              # 追踪器类型：linear | github | gitlab
  endpoint: ""              # API 端点（Linear 默认 https://api.linear.app/graphql，GitLab/GitHub 可自定义）
  api_key: $GITLAB_TOKEN    # API 密钥，支持 $VAR 语法引用环境变量
  project_slug: "org/repo"  # 项目标识（Linear: team key；GitHub/GitLab: namespace/repo）
  active_states:            # 触发 Agent 调度的 Issue 状态列表（原始形式）
    - Todo
    - In Progress
    - Merging
    - Rework
  terminal_states:          # 终态列表，进入终态的 Issue 会终止对应 worker
    - Closed
    - Cancelled
    - Canceled
    - Duplicate
    - Done
  workflow_labels:          # 工作流标签列表（GitHub/GitLab label-based 状态管理用）
    - Backlog
    - Human Review
```

**字段说明：**

| 字段 | 必填 | 默认值 | 说明 |
|------|------|--------|------|
| `kind` | 是 | — | 追踪器类型 |
| `endpoint` | 否 | 各平台默认值 | 自定义 API 端点（私有部署时使用） |
| `api_key` | 是 | — | API 密钥，推荐用 `$VAR` 引用环境变量 |
| `project_slug` | 是 | — | 项目标识 |
| `active_states` | 是 | `["Todo", "In Progress"]` | 活跃状态列表 |
| `terminal_states` | 是 | `["Closed", "Done", ...]` | 终态列表 |
| `workflow_labels` | 否 | `[]` | 工作流标签（GitHub/GitLab 用） |

---

### polling 块

控制 Tracker 轮询频率。

```yaml
polling:
  interval_ms: 5000   # 轮询间隔（毫秒），默认 30000
```

---

### workspace 块

配置工作空间根目录。

```yaml
workspace:
  root: ~/code/symphony-workspaces   # 工作空间根目录，支持 ~ 展开
```

每个 Issue 在 `root/<issue_id>/` 下拥有独立的工作空间目录。

---

### hooks 块

配置生命周期 Hook 脚本，在工作空间目录中以 shell 执行。

```yaml
hooks:
  after_create: |           # 工作空间创建后执行（通常用于 git clone）
    git clone --depth 1 https://github.com/org/repo .
  before_run: |             # 每次 Agent 运行前执行
    echo "before run"
  after_run: |              # 每次 Agent 运行后执行（优雅关闭时也会执行）
    echo "after run"
  before_remove: |          # 工作空间删除前执行（清理操作）
    make clean
  timeout_ms: 60000         # Hook 超时时间（毫秒），默认 60000
```

**字段说明：**

| 字段 | 必填 | 默认值 | 说明 |
|------|------|--------|------|
| `after_create` | 否 | — | 工作空间首次创建后执行，通常用于 clone 仓库 |
| `before_run` | 否 | — | 每轮 Agent 运行前执行 |
| `after_run` | 否 | — | 每轮 Agent 运行后执行，优雅关闭时也会触发 |
| `before_remove` | 否 | — | 工作空间 GC 删除前执行 |
| `timeout_ms` | 否 | `60000` | 单个 Hook 的超时时间 |

---

### agent 块

控制 Agent 调度行为。

```yaml
agent:
  max_concurrent_agents: 10              # 全局最大并发 Agent 数，默认 10
  max_turns: 20                          # 单次运行最大轮次，默认 20
  max_retry_backoff_ms: 300000           # 最大重试退避时间（毫秒），默认 300000（5分钟）
  max_concurrent_agents_by_state:        # 按状态限制并发数（可选）
    "In Progress": 5
    "Merging": 2
  blocker_check_states:                  # 检查阻塞依赖时考虑的状态列表（可选）
    - In Progress
    - Todo
```

**字段说明：**

| 字段 | 必填 | 默认值 | 说明 |
|------|------|--------|------|
| `max_concurrent_agents` | 否 | `10` | 全局并发上限 |
| `max_turns` | 否 | `20` | 单次运行最大轮次，超过后 worker 正常退出并等待续行 |
| `max_retry_backoff_ms` | 否 | `300000` | 指数退避重试的最大等待时间上限 |
| `max_concurrent_agents_by_state` | 否 | `{}` | 按 Issue 状态限制并发数 |
| `blocker_check_states` | 否 | `[]` | 调度前检查 blocked_by 依赖时，认为"阻塞中"的状态 |

---

### codex 块

配置 Codex app-server 子进程。

```yaml
codex:
  command: "codex app-server"            # 启动命令，默认 "codex app-server"
  approval_policy: never                 # 审批策略：never | auto | ...
  thread_sandbox: workspace-write        # 线程沙箱策略
  turn_sandbox_policy:                   # 轮次沙箱策略（可为字符串或对象）
    type: workspaceWrite
  turn_timeout_ms: 3600000               # 单轮超时（毫秒），默认 3600000（1小时）
  read_timeout_ms: 5000                  # 读取事件超时（毫秒），默认 5000
  stall_timeout_ms: 300000               # 停滞检测超时（毫秒），默认 300000（5分钟）
```

**字段说明：**

| 字段 | 必填 | 默认值 | 说明 |
|------|------|--------|------|
| `command` | 否 | `"codex app-server"` | Codex 启动命令，在工作空间目录中执行 |
| `approval_policy` | 否 | — | 传递给 Codex 的审批策略 |
| `thread_sandbox` | 否 | — | 线程级沙箱策略 |
| `turn_sandbox_policy` | 否 | — | 轮次级沙箱策略 |
| `turn_timeout_ms` | 否 | `3600000` | 单轮最大执行时间 |
| `read_timeout_ms` | 否 | `5000` | 从 Codex stdout 读取事件的超时 |
| `stall_timeout_ms` | 否 | `300000` | 无活动超时，超过后触发停滞检测并终止 worker |

---

### server 块

配置 rust-platform 内置 HTTP 服务器（可选，用于状态查询）。

```yaml
server:
  port: 8080   # HTTP 服务端口，不配置则不启动
```

---

### worker 块

配置 SSH 分布式 worker（可选）。

```yaml
worker:
  ssh_hosts:                             # SSH 主机列表
    - user@host1.example.com
    - user@host2.example.com
  max_concurrent_agents_per_host: 3      # 每台主机最大并发数
```

---

## 环境变量清单

| 变量 | 作用域 | 说明 |
|------|--------|------|
| `JWT_SECRET` | web-platform | JWT 签名密钥，至少 32 字符，**必填** |
| `ENCRYPTION_KEY` | web-platform | AES-GCM 256-bit 密钥（Base64），**必填** |
| `DATABASE_URL` | web-platform | SQLite 数据库路径，默认 `data.db` |
| `SERVER_HOST` | web-platform | 监听地址，默认 `0.0.0.0` |
| `SERVER_PORT` | web-platform | 监听端口，默认 `3000` |
| `SYMPHONY_BIN` | web-platform | symphony-platform 二进制路径 |
| `SYMPHONY_WORKSPACE_ROOT` | web-platform | 工作空间根目录 |
| `ADMIN_INIT_PASSWORD` | web-platform | 首次启动 admin 初始密码 |
| `RUST_LOG` | 两者 | 日志级别（tracing EnvFilter 格式） |
| `AZURE_OPENAI_BASEURL` | web-platform | AI 服务 API 端点（设置后启用 AI 功能） |
| `AZURE_OPENAI_API_KEY` | web-platform | AI 服务 API 密钥 |
| `AZURE_OPENAI_MODEL` | web-platform | AI 模型名称，默认 `gpt-5.5` |
| `AI_MODEL_FAMILY` | web-platform | 模型系列：`legacy` 或 `gpt5`（影响参数格式） |
| `AI_MAX_TOKENS` | web-platform | AI 生成最大 token 数，默认 `4096` |
| `AI_RATE_LIMIT_PER_MINUTE` | web-platform | 每用户每分钟 AI 请求限制，默认 `10` |
| `AI_GLOBAL_RATE_LIMIT_PER_MINUTE` | web-platform | 全局每分钟 AI 请求限制，默认 `30` |
| `http_proxy` / `HTTP_PROXY` | 两者 | HTTP 代理 |
| `https_proxy` / `HTTPS_PROXY` | 两者 | HTTPS 代理 |
| `all_proxy` / `ALL_PROXY` | 两者 | 全局代理 |
| `GITLAB_TOKEN` | rust-platform | GitLab API Token（通常通过 $VAR 在 WORKFLOW.md 中引用） |
| `GITHUB_TOKEN` | rust-platform | GitHub API Token |
| `LINEAR_API_KEY` | rust-platform | Linear API Key |
| `KANBAN_CACHE_TTL` | web-platform | Kanban 缓存 TTL（秒），默认 `10` |
| `ALERT_EVAL_INTERVAL_SECS` | web-platform | 告警评估间隔（秒），默认 `30` |
| `MAX_CONCURRENT_CODEX` | web-platform | 全局最大并发 Codex 进程数，默认 `5` |

---

## 热重载机制

rust-platform 使用 `notify` crate 监听 `WORKFLOW.md` 文件系统变更事件：

```
WORKFLOW.md 文件变更（inotify/FSEvents）
    │
    ▼
notify 事件触发
    │
    ▼
解析新配置（YAML front matter + 模板）
    │
    ├── 解析失败 → 输出 warn 日志，保留上次有效配置
    │
    └── 解析成功 → arc-swap 原子替换 ConfigHolder
                    │
                    ▼
                发送 ConfigReloaded 事件给 Orchestrator
                    │
                    ▼
                Orchestrator 更新 poll_interval_ms、max_concurrent_agents 等运行时参数
```

**重要约束：**

- 已在运行的 worker 持有启动时的配置快照，热重载不影响它们
- 新调度的 worker 使用最新配置
- 无效配置不会中断正在运行的系统

---

## 配置优先级

```
环境变量 > WORKFLOW.md 字段值 > 代码默认值
```

例如，`tracker.api_key: $GITLAB_TOKEN` 会在运行时替换为 `GITLAB_TOKEN` 环境变量的值。

---

## $VAR 语法

WORKFLOW.md 中的字符串值支持 `$VAR` 语法引用环境变量，在配置加载时展开：

```yaml
tracker:
  api_key: $GITLAB_TOKEN          # 引用 GITLAB_TOKEN 环境变量
  endpoint: $GITLAB_HOST          # 引用 GITLAB_HOST 环境变量

codex:
  command: $CODEX_COMMAND         # 引用自定义 Codex 命令
```

若引用的环境变量未设置，配置加载会失败并输出错误信息。

---

## 完整配置示例

### GitLab 配置

```yaml
---
tracker:
  kind: gitlab
  project_slug: "my-org/my-repo"
  active_states:
    - Todo
    - In Progress
    - Merging
    - Rework
  terminal_states:
    - Closed
    - Cancelled
    - Done
polling:
  interval_ms: 5000
workspace:
  root: ~/symphony-workspaces
hooks:
  after_create: |
    git clone --depth 1 https://gitlab.com/my-org/my-repo.git .
  timeout_ms: 120000
agent:
  max_concurrent_agents: 5
  max_turns: 30
  max_retry_backoff_ms: 600000
codex:
  command: codex --config shell_environment_policy.inherit=all app-server
  approval_policy: never
  stall_timeout_ms: 600000
---

You are working on GitLab issue {{ issue.identifier }}: {{ issue.title }}
...
```

### GitHub 配置

参见 `WORKFLOW.md.github`。

### Linear 配置

参见 `WORKFLOW.md.Linear.md`。
