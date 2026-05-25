# 数据库迁移策略文档

## Refinery 框架说明

项目使用 [Refinery](https://github.com/rust-db/refinery) 进行数据库迁移管理。

- **迁移执行时机**：web-platform 启动时自动执行，应用所有未执行的迁移
- **迁移文件位置**：`web-platform/migrations/`
- **版本追踪**：Refinery 在数据库中维护 `refinery_schema_history` 表，记录已执行的迁移版本
- **执行方式**：嵌入式（通过 `embed_migrations!` 宏将 SQL 文件编译进二进制）

---

## 迁移文件命名规范

```
V{NNN}__{description}.sql
```

- `V` 前缀固定
- `{NNN}` 为三位数字版本号，从 001 开始，严格递增
- `__` 为双下划线分隔符
- `{description}` 为小写字母和下划线组成的描述，简洁说明本次变更内容
- 文件扩展名为 `.sql`

**示例：**
- `V001__init_schema.sql`
- `V009__mr_create_idempotency.sql`

---

## 版本历史

### V001 — 初始 Schema（init_schema）

建立项目基础数据模型：

- `users`：用户账户表（id、username、password_hash、display_name、role、deleted_at）
- `user_configs`：用户平台配置表（gitlab_token、gitlab_host、github_token）
- `projects`：项目表（基础字段：name、git_url、platform、namespace、service_status 等）
- `project_members`：项目成员关联表
- `system_configs`：系统键值配置表，预置 3 条初始配置
- `token_blacklist`：JWT token 黑名单表
- `alert_history`：告警历史表（基础字段）

---

### V002 — 项目扩展字段（projects_extend）

为 `projects` 表新增 Phase 2 运行时字段：

- `max_concurrent_agents`：项目级最大并发 agent 数（默认 2）
- `auto_restart`：服务崩溃后是否自动重启（默认 1）
- `restart_count`：累计重启次数
- `last_started_at`、`last_stopped_at`：最后启停时间
- `error_message`：最后一次错误信息

---

### V003 — 并发控制表（phase4_concurrency）

新增 Phase 4 并发控制相关表：

- `concurrency_events`：并发事件历史记录（agent 启动/完成/限流事件）
- `concurrency_snapshots`：并发状态快照（用于断电恢复，每个项目一行）
- `system_configs` 新增 3 条并发相关配置

---

### V004 — 告警与通知（phase5_alerts）

新增 Phase 5 告警通知相关表：

- `alert_rules`：告警规则配置表，预置 6 条规则
- `notification_channels`：通知渠道配置表（支持 DingTalk 等）
- `alert_cooldowns`：告警冷却状态表
- `alert_history` 补充 `created_at` 字段
- 为 `alert_history` 补充多个查询优化索引
- `system_configs` 新增 3 条告警相关配置

---

### V005 — Workflow Hooks 与 Codex 配置（workflow_hooks_codex）

为 `projects` 表新增 Codex 和 Hooks 配置字段：

- `hooks_after_create`：工作区创建后执行的 shell 命令
- `hooks_before_remove`：工作区删除前执行的 shell 命令
- `codex_command`：Codex 启动命令（覆盖 WORKFLOW.md 默认值）
- `codex_approval_policy`：Codex 审批策略（默认 `never`）
- `codex_sandbox`：Codex 沙箱策略（默认 `workspace-write`）

---

### V006 — 恢复与生命周期围栏（resume_recovery_lifecycle）

为 `projects` 表新增服务生命周期围栏字段，支持多实例安全恢复：

- `web_instance_id`：发起操作的 web-platform 实例 ID
- `lifecycle_op_id`：当前生命周期操作 ID（幂等性）
- `lifecycle_lease_expires_at`：操作租约过期时间
- `service_owner_web_instance_id`：服务所有者实例 ID
- `service_owner_lease_expires_at`：所有者租约过期时间
- `service_owner_heartbeat_at`：所有者心跳时间
- `service_generation`：服务代次（每次启动递增）
- `service_instance_id`：服务实例 UUID
- `service_pgid`：进程组 ID
- `service_session_id`：会话 ID
- `service_cmdline_hash`：启动命令哈希（用于检测配置变更）
- `service_workdir`：服务工作目录
- `last_lifecycle_op`：最后一次生命周期操作类型

新增索引：`idx_projects_service_instance_id`、`idx_projects_service_owner`

---

### V007 — 网络代理配置（network_proxy）

新增网络代理支持：

- `secret_configs`：加密敏感配置表（存储代理 URL 等，AES-256-GCM 加密）
- `system_configs` 新增 4 条网络代理相关配置（mode、no_proxy、auto_bypass_local、version）

---

### V008 — 项目服务代理版本追踪（project_service_proxy_version）

为 `projects` 表新增代理版本追踪字段：

- `service_proxy_config_version`：服务启动时应用的代理配置版本

新增复合索引：`idx_projects_service_proxy_version ON projects(service_status, service_proxy_config_version)`

用途：检测代理配置变更后需要重启的服务实例。

---

### V009 — MR 创建幂等性（mr_create_idempotency）

新增 MR 创建幂等性支持：

- `idempotency_requests`：幂等性请求记录表（通过 `Idempotency-Key` 请求头去重）
- `merge_request_create_operations`：MR 创建操作记录表（追踪创建状态、平台 MR 信息）

两表通过 `operation_id` 关联，支持并发安全的 MR 创建去重。

---

## 编写新迁移的规范

### 只增不删原则

- **禁止**在迁移中删除列、删除表、重命名列
- 需要废弃字段时，保留字段并在应用层忽略，或新增替代字段
- 需要重命名时，新增目标字段，迁移数据，应用层切换，旧字段保留

### 向后兼容

- 新增列必须有默认值（`DEFAULT`）或允许 NULL，确保旧数据行不受影响
- 新增表不影响现有表结构
- 新增索引使用 `CREATE INDEX IF NOT EXISTS`

### 迁移文件要求

- 每个迁移文件只做一件事，保持原子性
- 文件顶部添加注释说明本次变更目的
- 预置数据使用 `INSERT OR IGNORE` 避免重复插入
- 补充已有表的字段使用 `ALTER TABLE ... ADD COLUMN`

### 测试要求

- 新迁移合并前，在干净数据库上执行全量迁移验证
- 在已有数据的数据库上执行增量迁移验证（不破坏现有数据）
- 验证新增索引不影响现有查询性能

---

## 回滚策略

SQLite 不支持原生的迁移回滚（`DOWN` 迁移）。Refinery 本身也不提供回滚机制。

**回滚方案：**

1. **数据库备份**：在执行迁移前备份数据库文件（`data.db`），出现问题时直接恢复备份
2. **手动逆向 SQL**：为每个迁移编写对应的逆向 SQL 脚本，存放在 `web-platform/migrations/rollback/` 目录（不被 Refinery 自动执行）
3. **新迁移修正**：若迁移已在生产环境执行，通过新版本迁移（V010+）修正问题，而非回滚

**逆向 SQL 示例（V009 回滚）：**

```sql
-- rollback/V009__mr_create_idempotency_rollback.sql
DROP INDEX IF EXISTS idx_mr_create_reconcile;
DROP INDEX IF EXISTS idx_mr_create_active_business;
DROP TABLE IF EXISTS merge_request_create_operations;
DROP INDEX IF EXISTS idx_idempotency_requests_operation;
DROP INDEX IF EXISTS idx_idempotency_requests_key;
DROP TABLE IF EXISTS idempotency_requests;
```

注意：回滚脚本需手动执行，且会丢失该版本写入的所有数据。
