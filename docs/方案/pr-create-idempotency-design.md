# PR 创建幂等机制方案

## 背景

当前 Web 平台已经具备 GitLab/GitHub 项目接入、Issue 创建、MR/PR 查询和看板展示能力，但 `GitPlatformClient` 还没有统一的“创建 MR/PR”能力。现有 `WORKFLOW.md` 要求交付前检查分支是否已有 PR，并规定 closed/merged PR 不可复用，需要新分支重新执行。这些规则目前主要依赖执行者按流程操作，缺少服务端幂等保护。

PR 创建是外部副作用操作。重复点击、前端超时重试、后端进程重启、网络超时、后台 agent 重入、同一 issue 被多个执行流同时推进，都可能导致重复创建、错误复用旧 PR 或状态不一致。本文设计服务端幂等机制，目标是在新增 PR 创建 API 或后台创建能力时，保证同一业务意图最多创建一个可复用的 open PR，并且所有重试都有确定结果。

## 目标

1. 对同一项目、同一源分支、同一目标分支、同一业务意图的 PR 创建请求提供幂等响应。
2. 支持前端和后台 agent 安全重试，重试返回同一个 PR、同一个进行中状态或同一个最终错误。
3. 在并发请求下只允许一个执行流真正调用 GitHub/GitLab 创建接口。
4. 外部平台调用成功但本地写库失败、请求超时或服务重启后，能够通过对账恢复。
5. 遵循现有 workflow：closed/merged PR 不作为本次创建的可复用结果。
6. 保持平台无关接口，GitHub PR 和 GitLab MR 使用同一业务模型。

## 非目标

- 不在本方案中实现自动建分支、自动 push 或代码生成。
- 第一阶段不支持 fork PR/MR；只支持源分支和目标分支都在项目配置的同一个仓库内。
- 不把不同源分支的 PR 视为同一业务意图，即使它们关联同一个 issue。
- 不尝试自动 reopen closed PR/MR。
- 不提供跨项目、跨仓库的全局幂等。
- 不依赖前端本地状态保证幂等，前端只负责传递幂等键和展示结果。

## 设计原则

采用三层幂等保护：

1. **请求幂等键**：每个 `Idempotency-Key` 都有独立请求记录、请求指纹和响应快照。
2. **业务操作锁**：同一业务键只允许一个 active create operation，防止不同请求 key 表达同一创建意图时重复创建。
3. **平台侧对账**：创建前后都按 source/target 查询外部平台；当创建接口超时或返回冲突时，通过查询恢复真实结果。

关键取舍：请求幂等记录和业务创建操作必须拆成两张表。单表同时承载 `idempotency_key` 和 `business_key` 会破坏多 key 同业务意图、hash mismatch、锁过期接管和历史 replay 语义。

## 第一阶段 fork 策略

第一阶段明确不支持 fork PR/MR。

请求体不得传 `source_project_path`、`source_owner`、`source_project_id` 等 fork 字段；如果未来 API 客户端传入这些字段，服务端返回 400。服务端构造平台查询时固定使用项目配置中的 `namespace/repo_name` 作为 source repo 和 target repo。

业务键、数据库和平台查询都按同仓库语义设计：

- GitHub `head` 固定为 `{project.namespace}:{source_branch}`。
- GitLab `source_project_id` 不传，使用当前项目内的 `source_branch`。

后续支持 fork 时，必须把 `source_project_path/source_owner/source_project_id` 纳入 API、request hash、business key、数据库列、平台 trait 和测试矩阵，不能在当前方案上隐式扩展。

## 业务唯一键

业务键使用版本化 canonical JSON 计算 hash，不使用字符串拼接。

canonical 输入：

```json
{
  "version": 1,
  "operation": "create_merge_request",
  "project_id": 1,
  "platform": "github",
  "project_path": "owner/repo",
  "source_project_path": "owner/repo",
  "source_branch": "codex/issue-123-login-fix",
  "target_branch": "main",
  "purpose_type": "issue_delivery",
  "purpose_id": "123"
}
```

字段说明：

| 字段 | 说明 |
|---|---|
| `project_id` | 本地项目 ID |
| `platform` | `github` 或 `gitlab`，显式纳入 hash |
| `project_path` | 目标仓库 `namespace/repo` |
| `source_project_path` | 第一阶段固定等于 `project_path` |
| `source_branch` | 源分支 |
| `target_branch` | 目标分支，省略时服务端补项目 `default_branch` 后再计算 |
| `purpose_type` | 创建来源，例如 `issue_delivery`、`manual`、`agent_handoff` |
| `purpose_id` | 业务对象 ID，例如 issue iid、任务 ID；手动创建可为空字符串 |

`title`、`description`、reviewers、labels 不进入业务键。这些字段可以在同一创建意图中变化，放入业务键会让“修改标题后重试”变成新建 PR。

## 请求规范化与 request_hash

`request_hash` 用于检测同一个 `Idempotency-Key` 是否被错误复用。它同样使用版本化 canonical JSON。

规范化规则：

| 字段 | 规则 |
|---|---|
| `source_branch` | 必填，trim 后不得为空；保留大小写和 `/`、`.`、`+` 等合法分支字符 |
| `target_branch` | 省略或空白时补项目 `default_branch`；补齐后参与 hash |
| `title` | 必填，trim；空或超过长度限制返回 400 |
| `description` | 省略、`null`、空白字符串统一为 `null`；非空保留正文 |
| `purpose_type` | 省略时默认为 `manual`；第一阶段限定为 `manual`、`issue_delivery`、`agent_handoff` |
| `purpose_id` | 省略或 `null` 统一为空字符串；非空时 trim 后参与 hash |
| `draft` | 省略时默认为 `false` |
| 未知字段 | 返回 400，不参与向后兼容 |

canonical JSON 必须按固定字段顺序生成，不能依赖 serde map 遍历顺序。前端显式传 `target_branch="main"` 与后台省略 `target_branch` 且项目默认分支为 `main` 时，hash 必须一致。

## API 设计

新增接口：

```http
POST /api/projects/:id/mrs
Idempotency-Key: <uuid-or-stable-key>
Content-Type: application/json
```

请求体：

```json
{
  "source_branch": "codex/issue-123-login-fix",
  "target_branch": "main",
  "title": "fix: login redirect",
  "description": "Closes #123",
  "purpose_type": "issue_delivery",
  "purpose_id": "123",
  "draft": false
}
```

成功或进行中都返回现有成功包装，HTTP 200，`retCode="0"`：

```json
{
  "data": {
    "operation_id": 42,
    "iid": 17,
    "state": "opened",
    "source_branch": "codex/issue-123-login-fix",
    "target_branch": "main",
    "web_url": "https://github.com/org/repo/pull/17",
    "idempotency_status": "created"
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

`in_progress` 也返回成功包装：

```json
{
  "data": {
    "operation_id": 42,
    "iid": null,
    "state": "creating",
    "source_branch": "codex/issue-123-login-fix",
    "target_branch": "main",
    "web_url": null,
    "idempotency_status": "in_progress",
    "retry_after_seconds": 2
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

`idempotency_status`：

| 值 | 含义 |
|---|---|
| `created` | 本次请求创建了新 PR |
| `replayed` | 命中相同 `Idempotency-Key`，返回已保存响应快照 |
| `reused_open` | 当前请求绑定到已有 open PR |
| `in_progress` | 另一个请求正在创建或对账，客户端用同一 key 重试 |
| `reconciled` | 上次外部调用结果不明，本次通过平台查询恢复 |

错误约定：

| 场景 | HTTP | retCode | 说明 |
|---|---:|---|---|
| 同一 `Idempotency-Key` 携带不同 `request_hash` | 409 | `BIZ_003` | 客户端误复用 key |
| 发现同源同目标只有 closed/merged PR，无 open PR | 409 | `BIZ_003` | 当前分支不可复用，需新建分支 |
| fork 字段或未知字段 | 400 | `BIZ_001` | 第一阶段不支持 |
| 平台 token 无效 | 400 | `TOKEN_001` | 复用现有错误语义 |
| 平台不可用且无法对账 | 502 | `EXT_001` | 请求记录保留为可重试 |

## 数据库设计

### `idempotency_requests`

每个请求 key 一行，保存 hash 和响应快照。

| 字段 | 说明 |
|---|---|
| `id` | 主键 |
| `project_id` | 本地项目 ID |
| `user_id` | 发起用户 ID |
| `idempotency_key` | 请求幂等键 |
| `request_hash` | 规范化请求体 hash |
| `operation_id` | 指向 `merge_request_create_operations.id`，可为空直到绑定 |
| `response_status` | `in_progress`、`succeeded`、`failed_final` |
| `http_status` | 终态 replay 时恢复原 HTTP 状态；进行中为 200 |
| `response_json` | 成功、进行中或最终错误的响应快照 |
| `created_at` / `updated_at` | UTC 时间戳 |

约束：

```sql
CREATE UNIQUE INDEX idx_idempotency_requests_key
ON idempotency_requests(project_id, user_id, idempotency_key);
```

`user_id` 纳入唯一约束，避免不同项目成员偶然或恶意复用同一 key 后拿到对方响应。

### `merge_request_create_operations`

每个业务创建意图一行，保存业务锁、平台结果和恢复状态。

| 字段 | 说明 |
|---|---|
| `id` | 主键 |
| `project_id` | 本地项目 ID |
| `platform` | `github` / `gitlab` |
| `project_path` | `namespace/repo` |
| `source_project_path` | 第一阶段固定等于 `project_path` |
| `business_key` | canonical business key hash |
| `business_key_json` | 脱敏后的 canonical JSON，便于诊断 |
| `source_branch` | 源分支 |
| `target_branch` | 目标分支 |
| `purpose_type` | 业务来源 |
| `purpose_id` | 业务对象 ID |
| `status` | `active`、`succeeded_open`、`succeeded_closed`、`failed_retryable`、`failed_final` |
| `platform_iid` | PR number / MR iid |
| `platform_node_id` | GitHub node_id 或 GitLab 全局 ID，可选 |
| `web_url` | PR/MR URL |
| `last_error_code` | 最近错误码 |
| `last_error_message` | 最近错误摘要，需脱敏 |
| `lock_owner_request_id` | 当前持锁的 request 行 ID |
| `locked_until` | UTC 时间戳 |
| `create_lease_token` | 当前创建执行实例的租约 token，可为空 |
| `create_lease_expires_at` | 创建租约过期时间，可为空 |
| `creation_started_at` | 最近一次真正调用平台创建接口的开始时间 |
| `created_at` / `updated_at` | UTC 时间戳 |

约束：

```sql
CREATE UNIQUE INDEX idx_mr_create_active_business
ON merge_request_create_operations(project_id, business_key)
WHERE status IN ('active', 'succeeded_open', 'failed_retryable');
```

`succeeded_closed` 和 `failed_final` 不占用 active business 唯一键。旧请求的 replay 由 `idempotency_requests.response_json` 保证，不依赖 operation 状态保持 active。

建议加约束：

```sql
CHECK(status IN ('active', 'succeeded_open', 'succeeded_closed', 'failed_retryable', 'failed_final'));
CHECK(business_key <> '');
CHECK(source_branch <> '');
CHECK(target_branch <> '');
```

## 状态机

operation 状态：

```text
new
  -> active

active
  -> succeeded_open      平台创建成功或对账发现 open PR
  -> failed_retryable    平台 5xx、网络超时、进程中断后无法确认
  -> failed_final        参数非法、分支不存在、无 diff、权限不足、只有 closed/merged PR

failed_retryable
  -> active              新请求或同 key 重试接管
  -> succeeded_open      对账恢复 open PR
  -> failed_final        对账确认不可恢复

succeeded_open
  -> succeeded_closed    对账发现外部 PR 已 closed/merged

succeeded_closed
  -> 终态，不再占用 active business key

failed_final
  -> 终态，不再占用 active business key
```

request 状态：

```text
new
  -> in_progress
  -> succeeded
  -> failed_final
```

同一个 `Idempotency-Key` 的 replay 规则：

- 如果 `response_json` 已经是 `succeeded` 或 `failed_final`，直接返回快照，不再重新判断外部 open/closed 状态。
- 这符合请求幂等语义：同一用户动作的重试返回同一结果。
- workflow 的 closed/merged 不复用由新请求或新业务操作触发时的平台对账保证；旧 key 的 replay 只是历史结果，不表示可以继续复用该 PR。

## 创建流程

所有数据库写锁阶段都必须是短事务；不得在 SQLite `IMMEDIATE` 事务内调用外部平台。

### 阶段 1：规范化与请求登记

1. 校验 JWT、项目成员权限、平台 token 配置存在性。
2. 解析项目，补齐 `target_branch`，拒绝 fork/未知字段。
3. 生成 canonical request JSON、`request_hash`、business key JSON 和 `business_key`。
4. 开启短事务。
5. 按 `(project_id, user_id, idempotency_key)` 查询 request：
   - 存在且 `request_hash` 不同：返回 409。
   - 存在且有终态 `response_json`：提交事务后直接 replay。
   - 存在且 `response_status=in_progress`：取 `operation_id`，提交事务后进入阶段 2 做只读对账；只有对账查不到 open PR/MR 且没有可用创建租约时才返回进行中。
   - 不存在：插入 request，`response_status=in_progress`。
6. 按 `business_key` 查询 active operation：
   - 存在 `active` 且锁未过期：把当前 request 绑定到该 operation，写入 `in_progress` 响应快照，提交事务，进入阶段 2 做只读对账；只有对账查不到 open PR/MR 时才返回 `in_progress`。
   - 存在 `failed_retryable` 或 `active` 锁过期：当前 request 接管 operation 锁，更新 `lock_owner_request_id` 和 `locked_until`，但仍不能直接创建，必须先进入阶段 2 对账。
   - 存在 `succeeded_open`：绑定当前 request，但不在事务内判断外部状态，进入阶段 2 对账。
   - 不存在：插入 operation 为 `active`，当前 request 持有 operation 锁，但创建租约尚未发放。
7. 提交事务。

operation 锁和创建租约是两个概念：

- `lock_owner_request_id` 表示哪个 request 可以在无 PR/MR 时申请创建租约。
- `create_lease_token` 表示哪个当前执行实例可以真正调用平台创建接口。
- 同一个 `Idempotency-Key` 的并发重试会命中同一 request 行，但不会自动拥有原执行实例的 `create_lease_token`。

### 阶段 2：平台预查与状态恢复

1. 如果 request 已有终态快照，直接返回。
2. 使用平台 client 按 source/target 查询 open PR/MR。
3. 如果找到 open PR：
   - 短事务写 operation 为 `succeeded_open`。
   - 当前 request 写 `succeeded` 响应快照，`idempotency_status=reused_open` 或 `reconciled`。
   - 失效相关缓存。
   - 返回成功。
4. 如果没有 open PR/MR，继续按 source/target 查询 closed/merged PR/MR：
   - GitHub 查询 `state=closed`，列表响应优先用 `merged_at != null` 区分 merged 与 closed；如果响应缺少 `merged_at`，再调用单个 PR detail 接口读取 `merged` 布尔值。
   - GitLab 查询 `state=closed` 和 `state=merged`，或查询 `state=all` 后本地按 source/target 和状态过滤。
5. 如果只找到 closed/merged PR：
   - 短事务写 operation 为 `failed_final` 或把旧 `succeeded_open` 转 `succeeded_closed` 后创建新的 `failed_final` 诊断记录。
   - 当前 request 写最终错误快照。
   - 返回 409，提示新建分支。
6. 如果没有任何 PR/MR，且当前 request 不是 operation 锁 owner：
   - 返回已保存的 `in_progress` 快照，建议 2 秒后重试。
7. 如果没有任何 PR/MR，且当前 request 是 operation 锁 owner：
   - 开启短事务尝试申请创建租约。
   - 若 `create_lease_token` 存在且未过期，说明另一个执行实例正在创建；提交事务，返回 `in_progress`。
   - 若 `create_lease_token` 为空或已过期，写入新的随机 `create_lease_token`、`create_lease_expires_at` 和 `creation_started_at`，提交事务，进入阶段 3 创建。

### 阶段 3：平台创建与后置对账

1. 只有持有当前有效 `create_lease_token` 的执行实例可以调用平台创建 PR/MR。平台 create 请求 timeout 必须小于创建租约 TTL。
2. 创建成功：
   - 短事务写 operation 为 `succeeded_open`。
   - 清空 `create_lease_token` 和 `create_lease_expires_at`。
   - 当前 request 写 `succeeded` 响应快照，`idempotency_status=created`。
   - 失效缓存。
   - 返回成功。
3. 创建返回“已存在 open PR/MR”或平台冲突：
   - 不直接报错，立即回到阶段 2 做只读对账。
4. 创建超时、连接中断或响应体无法确认：
   - 立即回到阶段 2 做只读对账。
   - 查到 open PR 写 `reconciled`。
   - 查不到则短事务写 operation 为 `failed_retryable`，清空创建租约，当前 request 保持 `in_progress`，本次返回 502；不要把 retryable 错误保存成终态 replay。
5. 创建返回 final 错误：
   - 短事务写 operation 为 `failed_final`。
   - 清空创建租约。
   - 当前 request 写最终错误快照。
   - 返回对应 400/403/409。

### 本地写库失败后的即时恢复

如果平台创建成功但写 `succeeded_open` 失败，下一次同 key 或同业务键请求不得只返回 `in_progress`。处理规则：

- 只要 request/operation 处于 `in_progress`、`active` 或 `failed_retryable`，重试都可以先执行只读平台对账。
- 对账查到 open PR 后立即补写 `succeeded_open` 和 request 终态快照。
- 锁未过期只禁止另一个请求再次创建，不禁止只读对账。
- 创建租约未过期时，任何并发重试都不得再次调用平台创建接口；只能对账或返回 `in_progress`。

## 平台客户端扩展

扩展 `GitPlatformClient`：

```rust
async fn create_merge_request(
    &self,
    token: &str,
    project_path: &str,
    req: &CreateMergeRequest,
) -> Result<PlatformMergeRequest, GitPlatformError>;

async fn find_open_merge_request_by_branches(
    &self,
    token: &str,
    project_path: &str,
    source_branch: &str,
    target_branch: &str,
) -> Result<Option<PlatformMergeRequest>, GitPlatformError>;

async fn find_merge_requests_by_branches(
    &self,
    token: &str,
    project_path: &str,
    source_branch: &str,
    target_branch: &str,
    states: &[MergeRequestState],
) -> Result<Vec<PlatformMergeRequest>, GitPlatformError>;
```

返回列表排序必须稳定：

1. open 优先。
2. `updated_at` 倒序。
3. `iid` 倒序。

`PlatformMergeRequest` 需要补充可选字段：

- `platform_node_id`
- `source_project_path`
- `target_project_path`

第一阶段 GitHub/GitLab 都填同一个项目路径；未来 fork 支持再扩展。

## 平台错误分类

现有 `GitPlatformError` 只有 `TokenInvalid`、`NotFound`、`ServiceUnavailable`、`RequestError`，不足以支撑创建状态机。需要扩展结构化错误：

```rust
pub enum GitPlatformError {
    TokenInvalid(String),
    Forbidden(String),
    NotFound(String),
    Validation {
        code: PlatformValidationCode,
        message: String,
    },
    Conflict {
        code: PlatformConflictCode,
        message: String,
    },
    ServiceUnavailable(String),
    RequestError(String),
}
```

第一阶段枚举值必须显式建模，不能只靠字符串匹配：

```rust
pub enum PlatformValidationCode {
    SourceBranchNotFound,
    TargetBranchNotFound,
    NoCommits,
    InvalidTitle,
    UnsupportedFork,
    Unknown,
}

pub enum PlatformConflictCode {
    ExistingOpenMergeRequest,
    ExistingClosedOrMergedMergeRequest,
    BranchProtectionOrPolicy,
    Unknown,
}
```

分类规则：

| 分类 | operation 状态 | HTTP |
|---|---|---:|
| token invalid | 不改变或 final | 400 `TOKEN_001` |
| forbidden | `failed_final` | 403 `AUTH_002` |
| source branch not found | `failed_final` | 400 `BIZ_001` |
| target branch not found | `failed_final` | 400 `BIZ_001` |
| no commits / no diff | `failed_final` | 409 `BIZ_003` |
| existing open PR/MR | 进入对账 | 200 成功或 409 |
| only closed/merged PR/MR | `failed_final` | 409 `BIZ_003` |
| rate limit / 5xx / timeout | `failed_retryable` | 502 `EXT_001` 或 429 |

GitHub：

- 创建接口使用 `POST /repos/{owner}/{repo}/pulls`。
- 预查 open 使用 `GET /repos/{owner}/{repo}/pulls?state=open&head={owner}:{source_branch}&base={target_branch}`。
- 必须用 `Url::query_pairs_mut()` 构造 query，不能字符串拼接。
- `head` 的 owner 固定来自项目 `namespace`；第一阶段拒绝 fork。
- GitHub 422 必须解析 body：已存在 PR 进入对账；分支不存在、无 commits、校验失败进入 final。

GitLab：

- 创建接口使用 `POST /projects/:id/merge_requests`。
- 预查 open 使用 `GET /projects/:id/merge_requests?state=opened&source_branch=...&target_branch=...`。
- project path 和 query 都必须使用 URL builder 或现有 encode 工具。
- GitLab 400/409/422 必须解析 body：已有 open MR 进入对账；分支不存在、无 diff、权限不足进入 final。
- 第一阶段不传 `source_project_id`；如果未来支持 fork，必须新增字段和测试。

## 并发策略

服务端以数据库唯一约束为主，不依赖内存锁。

`locked_until` 规则：

- 使用 UTC 时间。
- 默认锁有效期 2 分钟。
- 只有 `lock_owner_request_id` 对应 request 可以申请创建租约。
- 只有持有未过期 `create_lease_token` 的当前执行实例可以发起创建调用。
- 任何 request 都可以做只读平台对账。
- 锁过期后，新的 request 可以短事务接管 operation，但不能覆盖旧 request 行。
- 创建接口 timeout 必须小于创建租约 TTL；租约过期接管前必须先完成平台对账。

如果后续引入 PostgreSQL，可把短事务抢锁升级为 `SELECT ... FOR UPDATE SKIP LOCKED`，业务语义不变。

## 重试与对账

对账任务是实现目标的必需部分，不是可选建议。

触发点：

- API 请求看到 `active`、`failed_retryable`、`in_progress`。
- API 创建超时、连接中断或平台冲突。
- Web 平台启动后。
- 周期任务每 1 分钟扫描一次。

对账任务行为：

- 扫描 `active` 且 `locked_until < now` 的 operation。
- 扫描 `failed_retryable` 且 `updated_at < now - 1 minute` 的 operation。
- 每轮最多处理 50 条，避免压垮平台 API。
- 只做平台查询和状态恢复，不主动创建。
- token 选择优先使用 `lock_owner_request_id` 对应 request 的 `user_id`；若该用户 token 不可用，再尝试最近绑定到同一 operation 的 request 用户。
- token 不可用时记录脱敏错误，保持 `failed_retryable`，下次有用户上下文请求时再恢复。

对账结果：

| 结果 | 处理 |
|---|---|
| 平台已有 open PR | operation 写 `succeeded_open`，关联 in_progress request 写成功快照 |
| 平台只有 closed/merged PR | operation 写 `failed_final` 或 `succeeded_closed`，关联 request 写最终错误 |
| 平台无 PR，锁未过期 | request 返回 `in_progress` |
| 平台无 PR，锁过期 | 下一个有用户上下文的 request 可接管并创建 |
| 平台不可用 | 保持或写 `failed_retryable` |

## closed/merged PR 处理

规则分两层：

1. **旧 `Idempotency-Key` replay**：如果该 key 已保存成功快照，继续返回历史快照，`idempotency_status=replayed`。这是请求幂等语义，不代表可以继续复用该 PR。
2. **新请求或新业务操作**：必须重新对账平台状态。若同源同目标只存在 closed/merged PR，没有 open PR，则返回 409，提示新建分支。

operation 转换：

- 对账发现 `succeeded_open` 对应外部 PR 已 closed/merged 时，更新为 `succeeded_closed`，释放 active business 唯一键。
- 当前新 request 写 `failed_final` 响应快照，避免后续同 key 结果漂移。
- 不自动 reopen。

## 缓存影响

当前 `ApiCache` 只支持 `starts_with` 前缀失效。PR 创建成功或对账恢复成功后，必须失效以下可执行前缀：

| 缓存 | 当前 key 形态 | 失效方式 |
|---|---|---|
| 看板 | `{user_id}:{project_id}:kanban:{query_hash}` | `invalidate_prefix(&format!("{user_id}:{project_id}:kanban:"))` |
| Issue 详情 | `{user_id}:{project_id}:issue:{iid}:detail` | 精确 key 或前缀 |
| Issue 关联 MR/PR | `{user_id}:{project_id}:issue:{iid}:mrs` | 精确 key |
| MR/PR 详情 | `{user_id}:{project_id}:mr:{iid}:detail` | 精确 key |

仅失效当前用户不够。PR 是仓库级变化，项目其他成员也会看到。落地 PR 创建前必须先给 `ApiCache` 增加项目级缓存索引，或在写入缓存时同步维护 `project_id -> keys`，让创建成功后可以失效该项目所有成员的相关 key。

没有项目级失效能力时，PR 创建接口不得标记为完整可用，只能依赖 TTL 短暂兜底。

## 前端与后台调用

前端每次用户点击“创建 PR”时生成一个 UUID 作为 `Idempotency-Key`，请求重试期间复用同一个 key。

后台 agent 生成稳定 key：

```text
agent-run-id + project-id + issue-iid + source-branch + target-branch
```

规则：

- 同一个 agent run 重试复用同一 key。
- 新 source branch 使用新 key。
- 后台 agent 必须调用服务端 API 或同一 service 层，禁止绕过幂等机制直接执行 `gh pr create` / `glab mr create`。

交互：

- `created`、`replayed`、`reused_open`、`reconciled` 都按成功展示。
- `in_progress` 展示等待态，并用同一 key 重试。
- request hash mismatch 显示为客户端状态异常，要求刷新。
- closed/merged 冲突显示明确修复动作：新建分支后重试。

## 测试计划

后端请求幂等测试：

- 相同 key + 相同 normalized body 返回 `replayed`。
- 相同 key + 不同 `request_hash` 返回 409。
- 省略 `target_branch` 与显式默认分支 hash 一致。
- `description: null`、省略、空白字符串 hash 一致。
- 不同 key 命中同一 open PR 时，为新 key 写入响应快照；该 key 后续可 replay。
- 同一 key 首次成功后，外部 PR 被 close/merge，再次请求仍 replay 历史快照。

数据库和并发测试：

- 10 个不同 key、同一业务键并发，只产生一个 active operation 和一次平台创建。
- `failed_retryable` 被不同 key 接管，不覆盖旧 request。
- `active` 锁未过期时，新 key 返回 `in_progress`，但允许只读对账。
- `active` 锁未过期且平台已有 open PR 时，新 key 必须通过对账返回 `reused_open/reconciled`，不能直接返回 `in_progress`。
- 同一 key 并发重试命中同一 request 行时，只有持有有效 `create_lease_token` 的执行实例能调用平台创建。
- 平台创建成功但本地写成功失败后，下一次请求通过对账恢复。
- `succeeded_open` 对账为 closed 后转 `succeeded_closed` 并释放 active business key。

GitHub 平台测试：

- open PR 预查 URL 使用 query builder，断言 `head=owner:feature/foo`、`base=release/1.0`、分支含 `+` 时编码正确。
- fork 字段被拒绝，不会误用 base owner 查询 fork PR。
- GitHub 422 已存在 PR 进入对账。
- GitHub 422 分支不存在、无 commits 进入 final。
- GitHub closed/merged 预查使用 `state=closed&head=owner:branch&base=...`，并测试 `merged_at=null` 归一为 `closed`、`merged_at` 非空归一为 `merged`；若走 detail fallback，则测试 detail `merged` 布尔值。
- 平台返回冲突但二次查询只发现 closed/merged 时返回 409。

GitLab 平台测试：

- 创建 MR 请求体字段正确。
- `source_branch`、`target_branch` 查询使用 URL builder/encode。
- GitLab 400/409/422 已存在 open MR 进入对账。
- GitLab closed/merged 预查覆盖 `state=closed` 和 `state=merged`，或 `state=all` 后本地过滤。
- GitLab 分支不存在、无 diff、权限不足进入 final。
- 第一阶段 fork/source project 参数被拒绝。

缓存和集成测试：

- PR 创建成功后失效看板、issue detail、issue mrs、mr detail。
- 项目级缓存索引能失效其他成员缓存。
- 模拟服务在 `active` 后重启，启动对账恢复状态。
- 模拟平台创建成功但响应超时，第二次请求通过预查恢复。

## 落地步骤

1. 新增 `CreateMergeRequest` 请求/响应模型、canonical hash 工具和 `idempotency_status`。
2. 新增 `idempotency_requests` 与 `merge_request_create_operations` migration。
3. 增加 repository 短事务方法：登记 request、绑定 operation、抢锁、写响应快照、状态转换。
4. 扩展 `GitPlatformError`，实现平台错误结构化分类。
5. 扩展 `GitPlatformClient`，实现 GitHub/GitLab create 与 branch 查询。
6. 新增 `POST /api/projects/:id/mrs` handler，串联三阶段流程。
7. 增加项目级缓存索引和项目级失效能力。
8. 增加对账任务，启动时和周期性运行。
9. 增加后端、平台 mock、并发、缓存和前端测试。
10. 前端和后台 agent 统一接入该 API 或 service 层。

## 方案取舍

### 方案 A：只依赖前端 `Idempotency-Key`

实现简单，但无法覆盖后台 agent、不同客户端或刷新后重新生成 key 的场景，也无法处理平台调用成功但本地状态丢失。不采用。

### 方案 B：只依赖平台 head/base 预查

不需要本地表，但并发窗口仍可能同时查不到然后同时创建；也无法表达 request hash mismatch、pending、失败恢复和审计信息。不采用。

### 方案 C：单表同时记录 request 和 business lock

表结构简单，但会破坏多 key 同业务意图、锁过期接管和旧 key replay。第一轮对抗验证已证明不可行。不采用。

### 方案 D：请求幂等表 + 业务操作表 + 平台对账

实现成本中等，但能覆盖并发、重试、超时、服务重启、历史 replay 和平台冲突分类。采用。

## 风险与约束

- 第一阶段不支持 fork，避免 source repo 维度不清导致错误复用。后续支持必须成套扩展。
- 平台错误 body 解析必须有测试锁定，否则状态机会退化成 502 重试或错误 final。
- SQLite 并发能力有限，但 PR 创建频率低，短事务加唯一索引足够第一阶段使用。
- 后台 agent 如果直接执行 `gh pr create` 或 `glab mr create`，服务端幂等机制无法兜底，必须流程收口。
- 项目级缓存失效是可用性前置项，否则其他成员会短时间看到旧 PR 状态。

## 自查结论

- 请求幂等和业务操作锁已经拆表，避免单表语义冲突。
- `in_progress` 收敛为 HTTP 200 成功响应，不再混用 202 和 `BIZ_003`。
- closed/merged 分为旧 key replay 和新请求不可复用两层语义。
- 第一阶段明确不支持 fork，GitHub/GitLab 查询都按同仓库处理。
- 平台错误分类、缓存失效和测试计划已补齐到可实现粒度。
