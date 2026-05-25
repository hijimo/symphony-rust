# 数据库 Schema 文档

## 概述

- **数据库引擎**：SQLite 3
- **连接池**：r2d2（`r2d2_sqlite`），连接池大小可配置
- **迁移框架**：Refinery，应用启动时自动执行未应用的迁移
- **迁移文件路径**：`web-platform/migrations/`
- **当前版本**：V009

---

## 加密字段说明

以下字段在数据库中使用 **AES-256-GCM** 加密存储：

| 表 | 字段 | 说明 |
|----|------|------|
| `user_configs` | `gitlab_token`、`github_token` | 用户平台 token |
| `notification_channels` | `config_encrypted` | 通知渠道配置（含 webhook URL、密钥等） |
| `secret_configs` | `encrypted_value` | 网络代理 URL 等敏感配置 |

**加密算法**：AES-256-GCM，随机 nonce（12 字节），密文格式为 `base64(nonce || ciphertext)`。

**密钥来源**：环境变量 `ENCRYPTION_KEY`，值为 base64 编码的 32 字节密钥。启动时通过 `parse_base64_key()` 解析，若格式不正确则启动失败。

---

## 表结构

### users

用户账户表。

```sql
CREATE TABLE users (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    username    TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT,
    role        TEXT NOT NULL DEFAULT 'user',
    deleted_at  TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
```

| 字段 | 说明 |
|------|------|
| `id` | 自增主键 |
| `username` | 用户名，唯一 |
| `password_hash` | bcrypt 哈希密码 |
| `display_name` | 显示名称（可选） |
| `role` | 角色：`admin` 或 `user` |
| `deleted_at` | 软删除时间戳，NULL 表示未删除 |

**索引**：`idx_users_deleted_at ON users(deleted_at)`

---

### user_configs

用户级平台配置表，每个用户一行。

```sql
CREATE TABLE user_configs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER NOT NULL UNIQUE REFERENCES users(id),
    gitlab_token TEXT,
    gitlab_host TEXT,
    github_token TEXT,
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
```

| 字段 | 说明 |
|------|------|
| `user_id` | 关联用户，唯一（一对一） |
| `gitlab_token` | GitLab Personal Access Token（AES-256-GCM 加密） |
| `gitlab_host` | GitLab 实例地址（如 `https://gitlab.example.com`） |
| `github_token` | GitHub Personal Access Token（AES-256-GCM 加密） |

---

### projects

项目表，存储项目基本信息和服务运行状态。

```sql
CREATE TABLE projects (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    name                TEXT NOT NULL,
    description         TEXT,
    git_url             TEXT NOT NULL UNIQUE,
    platform            TEXT NOT NULL,
    platform_host       TEXT,
    namespace           TEXT NOT NULL,
    repo_name           TEXT NOT NULL,
    default_branch      TEXT DEFAULT 'main',
    workflow_template   TEXT NOT NULL DEFAULT 'default',
    workflow_content    TEXT,
    service_status      TEXT NOT NULL DEFAULT 'stopped',
    service_pid         INTEGER,
    created_by          INTEGER REFERENCES users(id),
    created_at          TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at          TEXT NOT NULL DEFAULT (datetime('now')),
    -- V002 新增
    max_concurrent_agents INTEGER NOT NULL DEFAULT 2,
    auto_restart        INTEGER NOT NULL DEFAULT 1,
    restart_count       INTEGER NOT NULL DEFAULT 0,
    last_started_at     TEXT,
    last_stopped_at     TEXT,
    error_message       TEXT,
    -- V005 新增
    hooks_after_create  TEXT,
    hooks_before_remove TEXT,
    codex_command       TEXT,
    codex_approval_policy TEXT DEFAULT 'never',
    codex_sandbox       TEXT DEFAULT 'workspace-write',
    -- V006 新增（生命周期围栏字段）
    web_instance_id     TEXT,
    lifecycle_op_id     TEXT,
    lifecycle_lease_expires_at TEXT,
    service_owner_web_instance_id TEXT,
    service_owner_lease_expires_at TEXT,
    service_owner_heartbeat_at TEXT,
    service_generation  INTEGER NOT NULL DEFAULT 0,
    service_instance_id TEXT,
    service_pgid        INTEGER,
    service_session_id  INTEGER,
    service_cmdline_hash TEXT,
    service_workdir     TEXT,
    last_lifecycle_op   TEXT,
    -- V008 新增
    service_proxy_config_version TEXT
);
```

**核心字段说明：**

| 字段 | 说明 |
|------|------|
| `git_url` | 仓库 URL，全局唯一 |
| `platform` | 平台类型：`gitlab` 或 `github` |
| `service_status` | 服务状态：`stopped`、`starting`、`running`、`stopping`、`error` |
| `service_pid` | 服务进程 PID |
| `service_generation` | 服务代次，每次启动递增，用于围栏检测 |
| `service_instance_id` | 服务实例 UUID，用于区分同名进程 |
| `service_pgid` | 进程组 ID，用于 kill 整个进程组 |
| `web_instance_id` | 发起操作的 web-platform 实例 ID |
| `lifecycle_op_id` | 当前生命周期操作 ID（用于幂等性） |
| `lifecycle_lease_expires_at` | 生命周期操作租约过期时间 |
| `service_proxy_config_version` | 启动时应用的代理配置版本 |

**索引：**
- `idx_projects_service_status ON projects(service_status)`
- `idx_projects_platform ON projects(platform)`
- `idx_projects_service_instance_id ON projects(service_instance_id)`
- `idx_projects_service_owner ON projects(service_owner_web_instance_id)`
- `idx_projects_service_proxy_version ON projects(service_status, service_proxy_config_version)`

---

### project_members

项目成员关联表。

```sql
CREATE TABLE project_members (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id     INTEGER NOT NULL REFERENCES users(id),
    role        TEXT NOT NULL DEFAULT 'member',
    synced_from TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(project_id, user_id)
);
```

| 字段 | 说明 |
|------|------|
| `role` | 成员角色：`owner` 或 `member` |
| `synced_from` | 同步来源（如 `gitlab`、`github`），手动添加时为 NULL |

**索引：**
- `idx_project_members_user ON project_members(user_id)`
- `idx_project_members_project ON project_members(project_id)`

---

### system_configs

系统级键值配置表。

```sql
CREATE TABLE system_configs (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    description TEXT,
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
```

预置配置项：

| key | 默认值 | 说明 |
|-----|--------|------|
| `max_concurrent_codex` | `5` | 全局最大 Codex 并行数 |
| `kanban_pending_limit` | `50` | 看板待处理 Issue 显示数量 |
| `kanban_done_days` | `7` | 看板已完成 Issue 回溯天数 |
| `concurrency_poll_interval_ms` | `5000` | 并发状态轮询间隔（毫秒） |
| `concurrency_heartbeat_timeout_s` | `30` | 心跳超时阈值（秒） |
| `concurrency_history_retention_days` | `30` | 并发事件历史保留天数 |
| `alert_enabled` | `true` | 全局告警开关 |
| `alert_evaluation_interval_seconds` | `30` | 规则评估间隔（秒） |
| `alert_history_retention_days` | `90` | 告警历史保留天数 |
| `network_proxy.mode` | `inherit_env` | 网络代理模式 |
| `network_proxy.no_proxy` | `` | 代理绕过规则 |
| `network_proxy.auto_bypass_local` | `true` | 自动绕过本机地址 |
| `network_proxy.version` | `1` | 代理配置版本 |

---

### token_blacklist

JWT token 黑名单表（用于强制登出）。

```sql
CREATE TABLE token_blacklist (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id         INTEGER NOT NULL REFERENCES users(id),
    invalidated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    reason          TEXT
);
```

**索引**：`idx_token_blacklist_user ON token_blacklist(user_id)`

---

### alert_rules

告警规则配置表（V004 创建）。

```sql
CREATE TABLE alert_rules (
    rule_id         TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL,
    severity        TEXT NOT NULL DEFAULT 'warning',
    enabled         INTEGER NOT NULL DEFAULT 1,
    threshold_json  TEXT NOT NULL DEFAULT '{}',
    cooldown_seconds INTEGER NOT NULL DEFAULT 300,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
```

预置规则：`task_timeout`、`task_failure`、`service_crash`、`concurrency_saturation`、`consecutive_failures`、`api_unreachable`

`severity` 枚举：`warning`、`critical`

---

### notification_channels

通知渠道配置表（V004 创建，表名为 `notification_channels`）。

```sql
CREATE TABLE notification_channels (
    channel_id          TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    channel_type        TEXT NOT NULL,
    enabled             INTEGER NOT NULL DEFAULT 1,
    config_encrypted    TEXT NOT NULL,
    severity_filter_json TEXT NOT NULL DEFAULT '["critical","warning"]',
    last_test_at        TEXT,
    last_test_success   INTEGER,
    created_at          TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at          TEXT NOT NULL DEFAULT (datetime('now'))
);
```

| 字段 | 说明 |
|------|------|
| `channel_type` | 渠道类型，如 `dingtalk` |
| `config_encrypted` | 渠道配置（AES-256-GCM 加密，含 webhook URL、签名密钥等） |
| `severity_filter_json` | 接收的告警级别过滤（JSON 数组） |

---

### alert_history

告警历史记录表（V001 创建，V004 补充字段和索引）。

```sql
CREATE TABLE alert_history (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id             TEXT NOT NULL,
    severity            TEXT NOT NULL,
    project_id          INTEGER REFERENCES projects(id),
    title               TEXT NOT NULL,
    message             TEXT NOT NULL,
    context_json        TEXT,
    fired_at            TEXT NOT NULL,
    resolved_at         TEXT,
    notified_at         TEXT,
    notification_channel TEXT,
    notification_status TEXT,
    created_at          TEXT NOT NULL DEFAULT (datetime('now'))
);
```

**索引：**
- `idx_alert_history_project ON alert_history(project_id)`
- `idx_alert_history_fired_at ON alert_history(fired_at)`
- `idx_alert_history_severity ON alert_history(severity)`
- `idx_alert_history_rule ON alert_history(rule_id)`
- `idx_alert_history_status ON alert_history(notification_status)`
- `idx_alert_history_fired_severity ON alert_history(fired_at, severity)`（分页查询优化）
- `idx_alert_history_project_fired ON alert_history(project_id, fired_at)`

---

### alert_cooldowns

告警冷却状态表（V004 创建，重启后恢复冷却状态）。

```sql
CREATE TABLE alert_cooldowns (
    rule_id     TEXT NOT NULL,
    scope_key   TEXT NOT NULL,
    last_fired_at TEXT NOT NULL,
    expires_at  TEXT NOT NULL,
    PRIMARY KEY (rule_id, scope_key)
);
```

**索引**：`idx_alert_cooldowns_expires ON alert_cooldowns(expires_at)`

---

### concurrency_events

并发事件历史记录表（V003 创建）。

```sql
CREATE TABLE concurrency_events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    event_type      TEXT NOT NULL,
    agent_id        TEXT,
    issue_iid       INTEGER,
    issue_title     TEXT,
    duration_seconds INTEGER,
    metadata_json   TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
```

`event_type` 枚举：`agent_started`、`agent_completed`、`throttle_on`、`throttle_off`

**索引：**
- `idx_concurrency_events_project ON concurrency_events(project_id)`
- `idx_concurrency_events_type ON concurrency_events(event_type)`
- `idx_concurrency_events_created_at ON concurrency_events(created_at)`
- `idx_concurrency_events_project_date ON concurrency_events(project_id, created_at)`（今日统计优化）

---

### concurrency_snapshots

并发状态快照表（V003 创建，用于断电恢复）。

```sql
CREATE TABLE concurrency_snapshots (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    active_agents INTEGER NOT NULL DEFAULT 0,
    queued_tasks INTEGER NOT NULL DEFAULT 0,
    agents_json TEXT,
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(project_id)
);
```

**索引**：`idx_concurrency_snapshots_updated ON concurrency_snapshots(updated_at)`

---

### secret_configs

加密敏感配置表（V007 创建，用于网络代理 URL 等）。

```sql
CREATE TABLE secret_configs (
    key             TEXT PRIMARY KEY,
    encrypted_value TEXT NOT NULL,
    kind            TEXT NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
```

`encrypted_value` 使用 AES-256-GCM 加密。

---

### idempotency_requests

MR 创建幂等性请求记录表（V009 创建）。

```sql
CREATE TABLE idempotency_requests (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    idempotency_key TEXT NOT NULL,
    request_hash    TEXT NOT NULL,
    operation_id    INTEGER REFERENCES merge_request_create_operations(id),
    response_status TEXT NOT NULL DEFAULT 'in_progress',
    http_status     INTEGER NOT NULL DEFAULT 200,
    response_json   TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK(response_status IN ('in_progress', 'succeeded', 'failed_final')),
    CHECK(idempotency_key <> ''),
    CHECK(request_hash <> '')
);
```

**索引：**
- `idx_idempotency_requests_key ON idempotency_requests(project_id, user_id, idempotency_key)`（唯一）
- `idx_idempotency_requests_operation ON idempotency_requests(operation_id)`

---

### merge_request_create_operations

MR 创建操作记录表（V009 创建）。

```sql
CREATE TABLE merge_request_create_operations (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id          INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    platform            TEXT NOT NULL,
    project_path        TEXT NOT NULL,
    source_project_path TEXT NOT NULL,
    business_key        TEXT NOT NULL,
    business_key_json   TEXT NOT NULL,
    source_branch       TEXT NOT NULL,
    target_branch       TEXT NOT NULL,
    purpose_type        TEXT NOT NULL,
    purpose_id          TEXT NOT NULL DEFAULT '',
    status              TEXT NOT NULL,
    platform_iid        INTEGER,
    platform_node_id    TEXT,
    web_url             TEXT,
    last_error_code     TEXT,
    last_error_message  TEXT,
    lock_owner_request_id INTEGER REFERENCES idempotency_requests(id),
    locked_until        TEXT NOT NULL,
    create_lease_token  TEXT,
    create_lease_expires_at TEXT,
    creation_started_at TEXT,
    created_at          TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at          TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK(status IN ('active', 'succeeded_open', 'succeeded_closed', 'failed_retryable', 'failed_final')),
    CHECK(business_key <> ''),
    CHECK(source_branch <> ''),
    CHECK(target_branch <> '')
);
```

**索引：**
- `idx_mr_create_active_business ON merge_request_create_operations(project_id, business_key) WHERE status IN ('active', 'succeeded_open', 'failed_retryable')`（部分唯一索引）
- `idx_mr_create_reconcile ON merge_request_create_operations(status, locked_until, updated_at)`

---

## ER 关系图（文字描述）

```
users (1) ──── (1) user_configs
  │
  ├── (1) ──── (N) project_members ──── (N) projects
  │                                          │
  │                                          ├── (1) ──── (N) concurrency_events
  │                                          ├── (1) ──── (1) concurrency_snapshots
  │                                          ├── (1) ──── (N) alert_history
  │                                          ├── (1) ──── (N) idempotency_requests
  │                                          └── (1) ──── (N) merge_request_create_operations
  │
  └── (1) ──── (N) token_blacklist

system_configs (独立键值表，无外键)
secret_configs (独立键值表，无外键)
alert_rules (独立表，无外键)
notification_channels (独立表，无外键)
alert_cooldowns (独立表，无外键)
```

**级联删除**：`project_members`、`concurrency_events`、`concurrency_snapshots`、`idempotency_requests`、`merge_request_create_operations` 在 `projects` 删除时级联删除（`ON DELETE CASCADE`）。
