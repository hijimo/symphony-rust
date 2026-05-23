# Symphony Web Platform API - Phase 5: 告警与通知 (Alert & Notification)

## 概述

Phase 5 实现预警引擎、通知分发器和告警管理功能。核心目标：

1. **预警引擎** — 指标采集 + 规则评估：任务超时、任务失败、服务异常、并行饱和、连续失败、API 不可达
2. **通知分发器框架** — NotificationChannel trait + 渠道路由
3. **钉钉群机器人通知** — DingTalk Webhook + HMAC-SHA256 签名
4. **告警历史与管理** — 告警历史 CRUD + 规则配置 + 渠道配置
5. **前端管理界面** — 告警历史 + 规则配置 + 渠道配置

## 通用协议

继承 Phase 1-4 所有通用协议，包括统一响应格式、错误码、认证方式、字段命名约定。

### 统一响应格式

```json
{ "data": T, "success": true, "retCode": "0", "retMsg": "ok" }
```

错误响应：
```json
{ "data": null, "success": false, "retCode": "CODE", "retMsg": "描述", "showType": N }
```

### 新增错误码

| retCode | 含义 | showType | HTTP Status |
|---------|------|----------|-------------|
| `ALERT_001` | 告警规则不存在 | 2 (error) | 404 |
| `ALERT_002` | 通知渠道配置无效 | 1 (warn) | 400 |
| `ALERT_003` | 测试通知发送失败 | 1 (warn) | 502 |

### 新增 WebPlatformError 变体

Phase 5 需要在 `web-platform/src/error.rs` 的 `WebPlatformError` 枚举中新增以下变体：

```rust
/// Alert rule not found (ALERT_001)
#[error("Alert rule not found: {0}")]
AlertRuleNotFound(String),  // -> HTTP 404, retCode "ALERT_001", showType 2

/// Notification channel config invalid (ALERT_002)
#[error("Channel config invalid: {0}")]
AlertChannelInvalid(String),  // -> HTTP 400, retCode "ALERT_002", showType 1

/// Test notification send failed (ALERT_003)
#[error("Notification send failed: {0}")]
AlertNotificationFailed(String),  // -> HTTP 502, retCode "ALERT_003", showType 1
```

### 速率限制（Phase 5 新增端点）

| 端点 | 限制 | 说明 |
|------|------|------|
| `GET /api/admin/alerts` | 60 次/分钟/用户 | 告警历史查询 |
| `GET /api/admin/alerts/rules` | 60 次/分钟/用户 | 规则配置查询 |
| `PUT /api/admin/alerts/rules` | 10 次/分钟/用户 | 规则配置变更 |
| `GET /api/admin/alerts/channels` | 60 次/分钟/用户 | 渠道配置查询 |
| `PUT /api/admin/alerts/channels` | 10 次/分钟/用户 | 渠道配置变更 |
| `POST /api/admin/alerts/test` | 3 次/分钟/用户 | 测试通知（防滥用） |

---

## 1. 告警历史列表

### 1.1 GET /api/admin/alerts

获取告警历史列表，支持分页和多维度筛选。

**认证**: Bearer Token (JWT)
**权限**: Admin

**查询参数**:

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| page_no | i64 | 1 | 页码 |
| page_size | i64 | 20 | 每页数量 (1-100) |
| severity | string | - | 按严重级别筛选：`critical` / `warning` / `info` |
| rule_id | string | - | 按规则 ID 筛选 |
| project_id | i64 | - | 按项目 ID 筛选 |
| status | string | - | 按通知状态筛选：`sent` / `failed` / `suppressed` |
| start_time | string | - | 起始时间（ISO 8601） |
| end_time | string | - | 结束时间（ISO 8601） |

**成功响应** (200):
```json
{
  "data": {
    "limit": 20,
    "offset": 0,
    "pageNo": 1,
    "pageSize": 20,
    "pages": 5,
    "records": [
      {
        "id": 1,
        "ruleId": "task_timeout",
        "severity": "warning",
        "projectId": 1,
        "projectName": "my-app",
        "title": "任务超时告警",
        "message": "项目 my-app 的 Issue #42 (修复登录Bug) 运行时间已超过 30 分钟（当前 35 分钟）",
        "context": {
          "issue_iid": "42",
          "issue_title": "修复登录Bug",
          "duration_minutes": "35",
          "threshold_minutes": "30"
        },
        "firedAt": "2026-05-20T14:30:00Z",
        "resolvedAt": null,
        "notifiedAt": "2026-05-20T14:30:01Z",
        "notificationChannel": "dingtalk",
        "notificationStatus": "sent"
      },
      {
        "id": 2,
        "ruleId": "service_crash",
        "severity": "critical",
        "projectId": 2,
        "projectName": "backend-service",
        "title": "服务异常退出",
        "message": "项目 backend-service 的 Symphony 实例进程意外退出 (exit code: 137)",
        "context": {
          "exit_code": "137",
          "pid": "12345",
          "uptime_seconds": "3600"
        },
        "firedAt": "2026-05-20T15:00:00Z",
        "resolvedAt": null,
        "notifiedAt": "2026-05-20T15:00:02Z",
        "notificationChannel": "dingtalk",
        "notificationStatus": "sent"
      }
    ],
    "totalCount": 95
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| id | i64 | 告警记录 ID |
| ruleId | string | 触发的规则标识 |
| severity | string | 严重级别：`critical` / `warning` / `info` |
| projectId | i64/null | 关联项目 ID（系统级告警为 null） |
| projectName | string/null | 关联项目名称 |
| title | string | 告警标题 |
| message | string | 告警详细描述 |
| context | object/null | 告警上下文键值对 |
| firedAt | string | 告警触发时间（ISO 8601） |
| resolvedAt | string/null | 告警解除时间 |
| notifiedAt | string/null | 通知发送时间 |
| notificationChannel | string/null | 通知渠道 |
| notificationStatus | string/null | 通知状态：`sent` / `failed` / `suppressed` |

**错误响应**:
- `AUTH_001` (401): 未认证
- `AUTH_002` (403): 非 Admin
- `BIZ_001` (400): 查询参数无效

**Rust 数据模型**:
```rust
#[derive(Debug, Deserialize, ToSchema)]
pub struct AlertHistoryQuery {
    pub page_no: Option<i64>,
    pub page_size: Option<i64>,
    pub severity: Option<String>,
    pub rule_id: Option<String>,
    pub project_id: Option<i64>,
    pub status: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertHistoryRecord {
    pub id: i64,
    pub rule_id: String,
    pub severity: String,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
    pub title: String,
    pub message: String,
    pub context: Option<HashMap<String, String>>,
    pub fired_at: String,
    pub resolved_at: Option<String>,
    pub notified_at: Option<String>,
    pub notification_channel: Option<String>,
    pub notification_status: Option<String>,
}

// NOTE: Use existing PaginationData<AlertHistoryRecord> from models/response.rs
// instead of a custom struct. PaginationData uses i64 for all numeric fields
// and provides the standard pagination format (limit, offset, pageNo, pageSize, pages, totalCount).
// Handler return type: ResponseData<PaginationData<AlertHistoryRecord>>
```

---

## 2. 告警规则配置

### 2.1 GET /api/admin/alerts/rules

获取所有告警规则的当前配置。

**认证**: Bearer Token (JWT)
**权限**: Admin

**成功响应** (200):
```json
{
  "data": {
    "rules": [
      {
        "ruleId": "task_timeout",
        "name": "任务超时",
        "description": "Codex 单任务运行时间超过阈值时触发",
        "severity": "warning",
        "enabled": true,
        "threshold": {
          "timeout_minutes": 30
        },
        "cooldownSeconds": 300,
        "updatedAt": "2026-05-20T10:00:00Z"
      },
      {
        "ruleId": "task_failure",
        "name": "任务失败",
        "description": "Codex 任务异常退出且重试耗尽时触发",
        "severity": "critical",
        "enabled": true,
        "threshold": {},
        "cooldownSeconds": 300,
        "updatedAt": "2026-05-20T10:00:00Z"
      },
      {
        "ruleId": "service_crash",
        "name": "服务异常退出",
        "description": "Symphony 实例进程意外退出时触发",
        "severity": "critical",
        "enabled": true,
        "threshold": {},
        "cooldownSeconds": 300,
        "updatedAt": "2026-05-20T10:00:00Z"
      },
      {
        "ruleId": "concurrency_saturation",
        "name": "并行饱和",
        "description": "全局并行数达到上限持续超过阈值时间时触发",
        "severity": "warning",
        "enabled": true,
        "threshold": {
          "saturation_minutes": 10
        },
        "cooldownSeconds": 600,
        "updatedAt": "2026-05-20T10:00:00Z"
      },
      {
        "ruleId": "consecutive_failures",
        "name": "连续失败",
        "description": "同一项目连续 N 个任务失败时触发",
        "severity": "critical",
        "enabled": true,
        "threshold": {
          "failure_count": 3
        },
        "cooldownSeconds": 300,
        "updatedAt": "2026-05-20T10:00:00Z"
      },
      {
        "ruleId": "api_unreachable",
        "name": "API 不可达",
        "description": "GitLab/GitHub API 连续请求失败时触发",
        "severity": "critical",
        "enabled": true,
        "threshold": {
          "failure_count": 5
        },
        "cooldownSeconds": 600,
        "updatedAt": "2026-05-20T10:00:00Z"
      }
    ]
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| ruleId | string | 规则唯一标识 |
| name | string | 规则显示名称 |
| description | string | 规则描述 |
| severity | string | 告警严重级别：`critical` / `warning` / `info` |
| enabled | bool | 是否启用 |
| threshold | object | 规则阈值参数（不同规则有不同字段） |
| cooldownSeconds | i64 | 冷却时间（秒），同一规则在此时间内不重复触发 |
| updatedAt | string | 最后更新时间（ISO 8601） |

**threshold 字段说明（按规则）**:

| rule_id | threshold 字段 | 类型 | 默认值 | 说明 |
|---------|---------------|------|--------|------|
| task_timeout | timeout_minutes | i64 | 30 | 任务超时阈值（分钟） |
| task_failure | (无) | - | - | 重试耗尽即触发，无额外阈值 |
| service_crash | (无) | - | - | 进程异常退出即触发 |
| concurrency_saturation | saturation_minutes | i64 | 10 | 饱和持续时间阈值（分钟） |
| consecutive_failures | failure_count | i64 | 3 | 连续失败次数阈值 |
| api_unreachable | failure_count | i64 | 5 | 连续 API 失败次数阈值 |

**错误响应**:
- `AUTH_001` (401): 未认证
- `AUTH_002` (403): 非 Admin

**Rust 数据模型**:
```rust
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertRule {
    pub rule_id: String,
    pub name: String,
    pub description: String,
    pub severity: String,
    pub enabled: bool,
    pub threshold: HashMap<String, serde_json::Value>,
    pub cooldown_seconds: i64,
    pub updated_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertRulesResponse {
    pub rules: Vec<AlertRule>,
}
```

---

### 2.2 PUT /api/admin/alerts/rules

批量更新告警规则配置（启用/禁用、阈值、冷却时间）。

**认证**: Bearer Token (JWT)
**权限**: Admin

**请求体**:
```json
{
  "rules": [
    {
      "ruleId": "task_timeout",
      "enabled": true,
      "threshold": {
        "timeout_minutes": 45
      },
      "cooldownSeconds": 600
    },
    {
      "ruleId": "consecutive_failures",
      "enabled": false
    }
  ]
}
```

**请求字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| rules | array | 是 | 要更新的规则列表 |
| rules[].ruleId | string | 是 | 规则标识（必须是已知规则） |
| rules[].enabled | bool | 否 | 是否启用（不传则不修改） |
| rules[].threshold | object | 否 | 阈值参数（不传则不修改） |
| rules[].cooldownSeconds | i64 | 否 | 冷却时间（不传则不修改，范围 60-3600） |

**成功响应** (200):
```json
{
  "data": {
    "updatedCount": 2,
    "rules": [
      {
        "ruleId": "task_timeout",
        "name": "任务超时",
        "description": "Codex 单任务运行时间超过阈值时触发",
        "severity": "warning",
        "enabled": true,
        "threshold": {
          "timeout_minutes": 45
        },
        "cooldownSeconds": 600,
        "updatedAt": "2026-05-22T10:30:00Z"
      },
      {
        "ruleId": "consecutive_failures",
        "name": "连续失败",
        "description": "同一项目连续 N 个任务失败时触发",
        "severity": "critical",
        "enabled": false,
        "threshold": {
          "failure_count": 3
        },
        "cooldownSeconds": 300,
        "updatedAt": "2026-05-22T10:30:00Z"
      }
    ]
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**校验规则**:
- `ruleId` 必须是系统已知的规则标识，否则返回 `ALERT_001`
- `cooldownSeconds` 范围 [60, 3600]，超出返回 `BIZ_001`
- `threshold` 中的字段必须符合对应规则的 schema，否则返回 `BIZ_001`
- 部分更新：只修改请求中包含的字段，未包含的字段保持不变

**错误响应**:
- `AUTH_001` (401): 未认证
- `AUTH_002` (403): 非 Admin
- `BIZ_001` (400): 参数校验失败（cooldown 超出范围、threshold 字段无效）
- `ALERT_001` (404): 规则 ID 不存在

**Rust 数据模型**:
```rust
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlertRulesRequest {
    pub rules: Vec<UpdateAlertRuleItem>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlertRuleItem {
    pub rule_id: String,
    pub enabled: Option<bool>,
    pub threshold: Option<HashMap<String, serde_json::Value>>,
    pub cooldown_seconds: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlertRulesResponse {
    pub updated_count: u32,
    pub rules: Vec<AlertRule>,
}
```

---

## 3. 通知渠道配置

### 3.1 GET /api/admin/alerts/channels

获取所有通知渠道的当前配置。

**认证**: Bearer Token (JWT)
**权限**: Admin

**成功响应** (200):
```json
{
  "data": {
    "channels": [
      {
        "channelId": "dingtalk",
        "name": "钉钉群机器人",
        "channelType": "dingtalk",
        "enabled": true,
        "config": {
          "webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=****xxxx",
          "secret": "****xxxx"
        },
        "configMasked": true,
        "severityFilter": ["critical", "warning"],
        "lastTestAt": "2026-05-20T10:00:00Z",
        "lastTestSuccess": true,
        "updatedAt": "2026-05-20T10:00:00Z"
      }
    ],
    "availableTypes": [
      {
        "type": "dingtalk",
        "name": "钉钉群机器人",
        "configSchema": {
          "webhook_url": { "type": "string", "required": true, "description": "钉钉 Webhook URL" },
          "secret": { "type": "string", "required": false, "description": "加签密钥（HMAC-SHA256）" }
        }
      },
      {
        "type": "slack",
        "name": "Slack",
        "configSchema": {
          "webhook_url": { "type": "string", "required": true, "description": "Slack Incoming Webhook URL" }
        },
        "status": "coming_soon"
      },
      {
        "type": "email",
        "name": "邮件",
        "configSchema": {
          "smtp_host": { "type": "string", "required": true, "description": "SMTP 服务器地址" },
          "smtp_port": { "type": "number", "required": true, "description": "SMTP 端口" },
          "username": { "type": "string", "required": true, "description": "SMTP 用户名" },
          "password": { "type": "string", "required": true, "description": "SMTP 密码" },
          "recipients": { "type": "array", "required": true, "description": "收件人列表" }
        },
        "status": "coming_soon"
      },
      {
        "type": "webhook",
        "name": "自定义 Webhook",
        "configSchema": {
          "url": { "type": "string", "required": true, "description": "Webhook URL" },
          "headers": { "type": "object", "required": false, "description": "自定义请求头" }
        },
        "status": "coming_soon"
      }
    ]
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| channels[] | array | 已配置的通知渠道列表 |
| channels[].channelId | string | 渠道唯一标识 |
| channels[].name | string | 渠道显示名称 |
| channels[].channelType | string | 渠道类型：`dingtalk` / `slack` / `email` / `webhook` |
| channels[].enabled | bool | 是否启用 |
| channels[].config | object | 渠道配置（敏感字段脱敏显示） |
| channels[].configMasked | bool | 配置是否已脱敏 |
| channels[].severityFilter | array | 接收的告警级别列表 |
| channels[].lastTestAt | string/null | 最后测试时间 |
| channels[].lastTestSuccess | bool/null | 最后测试是否成功 |
| channels[].updatedAt | string | 最后更新时间 |
| availableTypes[] | array | 系统支持的渠道类型列表（含配置 schema） |
| availableTypes[].status | string | 渠道状态：无此字段表示可用，`coming_soon` 表示即将支持 |

**脱敏规则**:
- `webhook_url`: 保留前 40 字符 + `****` + 最后 4 字符
- `secret`: 显示为 `****` + 最后 4 字符
- `password`: 显示为 `****`

**错误响应**:
- `AUTH_001` (401): 未认证
- `AUTH_002` (403): 非 Admin

**Rust 数据模型**:
```rust
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotificationChannelConfig {
    pub channel_id: String,
    pub name: String,
    pub channel_type: String,
    pub enabled: bool,
    pub config: HashMap<String, serde_json::Value>,
    pub config_masked: bool,
    pub severity_filter: Vec<String>,
    pub last_test_at: Option<String>,
    pub last_test_success: Option<bool>,
    pub updated_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChannelTypeInfo {
    #[serde(rename = "type")]
    pub channel_type: String,
    pub name: String,
    pub config_schema: HashMap<String, ConfigFieldSchema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFieldSchema {
    #[serde(rename = "type")]
    pub field_type: String,
    pub required: bool,
    pub description: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertChannelsResponse {
    pub channels: Vec<NotificationChannelConfig>,
    pub available_types: Vec<ChannelTypeInfo>,
}
```

---

### 3.2 PUT /api/admin/alerts/channels

更新通知渠道配置（新增、修改或删除渠道）。

**认证**: Bearer Token (JWT)
**权限**: Admin

**请求体**:
```json
{
  "channels": [
    {
      "channelId": "dingtalk",
      "name": "钉钉群机器人",
      "channelType": "dingtalk",
      "enabled": true,
      "config": {
        "webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=abc123def456",
        "secret": "SEC1234567890abcdef"
      },
      "severityFilter": ["critical", "warning"]
    }
  ]
}
```

**请求字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| channels | array | 是 | 渠道配置列表（全量替换） |
| channels[].channelId | string | 否 | 渠道 ID（不传则自动生成，用于新增） |
| channels[].name | string | 是 | 渠道显示名称 |
| channels[].channelType | string | 是 | 渠道类型（必须是 availableTypes 中的类型） |
| channels[].enabled | bool | 是 | 是否启用 |
| channels[].config | object | 是 | 渠道配置（明文，服务端加密存储） |
| channels[].severityFilter | array | 是 | 接收的告警级别列表 |

**成功响应** (200):
```json
{
  "data": {
    "channels": [
      {
        "channelId": "dingtalk",
        "name": "钉钉群机器人",
        "channelType": "dingtalk",
        "enabled": true,
        "config": {
          "webhook_url": "https://oapi.dingtalk.com/robot/send****f456",
          "secret": "****cdef"
        },
        "configMasked": true,
        "severityFilter": ["critical", "warning"],
        "lastTestAt": null,
        "lastTestSuccess": null,
        "updatedAt": "2026-05-22T10:30:00Z"
      }
    ]
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**校验规则**:
- `channelType` 必须是系统支持的类型（当前仅 `dingtalk`），否则返回 `ALERT_002`
- `config` 必须包含对应 channelType 的所有 required 字段，否则返回 `ALERT_002`
- `severityFilter` 中的值必须是 `critical` / `warning` / `info`，否则返回 `BIZ_001`
- 钉钉 `webhook_url` 必须以 `https://oapi.dingtalk.com/robot/send` 开头，否则返回 `ALERT_002`
- 配置中的敏感字段（webhook_url, secret）使用 AES-256-GCM 加密存储

**错误响应**:
- `AUTH_001` (401): 未认证
- `AUTH_002` (403): 非 Admin
- `BIZ_001` (400): severityFilter 值无效
- `ALERT_002` (400): 渠道配置无效（类型不支持、必填字段缺失、URL 格式错误）

**Rust 数据模型**:
```rust
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlertChannelsRequest {
    pub channels: Vec<UpdateChannelItem>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateChannelItem {
    pub channel_id: Option<String>,
    pub name: String,
    pub channel_type: String,
    pub enabled: bool,
    pub config: HashMap<String, serde_json::Value>,
    pub severity_filter: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlertChannelsResponse {
    pub channels: Vec<NotificationChannelConfig>,
}
```

---

## 4. 测试通知

### 4.1 POST /api/admin/alerts/test

发送测试通知，验证通知渠道连通性。

**认证**: Bearer Token (JWT)
**权限**: Admin

**请求体**:
```json
{
  "channelId": "dingtalk",
  "message": "这是一条测试通知"
}
```

**请求字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| channelId | string | 是 | 要测试的渠道 ID |
| message | string | 否 | 自定义测试消息（不传则使用默认测试消息） |

**成功响应** (200):
```json
{
  "data": {
    "success": true,
    "channelId": "dingtalk",
    "channelType": "dingtalk",
    "sentAt": "2026-05-22T10:30:00Z",
    "responseTimeMs": 235
  },
  "success": true,
  "retCode": "0",
  "retMsg": "ok"
}
```

**发送失败响应** (502):
```json
{
  "data": null,
  "success": false,
  "retCode": "ALERT_003",
  "retMsg": "测试通知发送失败: DingTalk API returned 400 - invalid webhook token",
  "showType": 1
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| success | bool | 发送是否成功 |
| channelId | string | 测试的渠道 ID |
| channelType | string | 渠道类型 |
| sentAt | string | 发送时间（ISO 8601） |
| responseTimeMs | u64 | 渠道响应时间（毫秒） |

**测试消息格式（钉钉）**:
```json
{
  "msgtype": "markdown",
  "markdown": {
    "title": "Symphony 通知测试",
    "text": "### Symphony 通知测试\n\n**状态**: 连通性验证成功\n\n**渠道**: 钉钉群机器人\n\n**时间**: 2026-05-22 10:30:00\n\n**操作人**: admin\n\n---\n\n自定义消息: 这是一条测试通知"
  }
}
```

**校验规则**:
- `channelId` 必须是已配置的渠道，否则返回 `ALERT_002`
- 渠道必须处于启用状态，否则返回 `ALERT_002`（retMsg 提示渠道已禁用）
- 测试发送超时时间：10 秒
- 测试结果会更新渠道的 `lastTestAt` 和 `lastTestSuccess` 字段

**错误响应**:
- `AUTH_001` (401): 未认证
- `AUTH_002` (403): 非 Admin
- `ALERT_002` (400): 渠道不存在或已禁用
- `ALERT_003` (502): 通知发送失败（含具体错误信息）

**Rust 数据模型**:
```rust
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestNotificationRequest {
    pub channel_id: String,
    pub message: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestNotificationResponse {
    pub success: bool,
    pub channel_id: String,
    pub channel_type: String,
    pub sent_at: String,
    pub response_time_ms: u64,
}
```

---

## 5. 数据库 Schema 变更

### 5.1 新增迁移文件: V004__phase5_alerts.sql

> **注意**: 当前已有迁移为 V001、V002、V003。`alert_history` 表已在 V001 中创建（含基础字段和部分索引），本迁移仅补充缺失的列和索引。

```sql
-- 告警规则配置表
CREATE TABLE alert_rules (
    rule_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'warning',  -- 'critical' | 'warning' | 'info'
    enabled INTEGER NOT NULL DEFAULT 1,        -- 0=disabled, 1=enabled
    threshold_json TEXT NOT NULL DEFAULT '{}', -- JSON: rule-specific threshold params
    cooldown_seconds INTEGER NOT NULL DEFAULT 300,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 预置告警规则
INSERT INTO alert_rules (rule_id, name, description, severity, enabled, threshold_json, cooldown_seconds) VALUES
('task_timeout', '任务超时', 'Codex 单任务运行时间超过阈值时触发', 'warning', 1, '{"timeout_minutes":30}', 300),
('task_failure', '任务失败', 'Codex 任务异常退出且重试耗尽时触发', 'critical', 1, '{}', 300),
('service_crash', '服务异常退出', 'Symphony 实例进程意外退出时触发', 'critical', 1, '{}', 300),
('concurrency_saturation', '并行饱和', '全局并行数达到上限持续超过阈值时间时触发', 'warning', 1, '{"saturation_minutes":10}', 600),
('consecutive_failures', '连续失败', '同一项目连续 N 个任务失败时触发', 'critical', 1, '{"failure_count":3}', 300),
('api_unreachable', 'API 不可达', 'GitLab/GitHub API 连续请求失败时触发', 'critical', 1, '{"failure_count":5}', 600);

-- 通知渠道配置表
CREATE TABLE notification_channels (
    channel_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    channel_type TEXT NOT NULL,               -- 'dingtalk' | 'slack' | 'email' | 'webhook'
    enabled INTEGER NOT NULL DEFAULT 1,
    config_encrypted TEXT NOT NULL,            -- AES-256-GCM encrypted JSON config
    severity_filter_json TEXT NOT NULL DEFAULT '["critical","warning"]',
    last_test_at TEXT,
    last_test_success INTEGER,                -- 0=failed, 1=success, NULL=never tested
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- alert_history 表已在 V001 中创建，此处补充 created_at 列和额外索引
ALTER TABLE alert_history ADD COLUMN created_at TEXT NOT NULL DEFAULT (datetime('now'));

-- 补充 V001 中未创建的索引
CREATE INDEX IF NOT EXISTS idx_alert_history_rule ON alert_history(rule_id);
CREATE INDEX IF NOT EXISTS idx_alert_history_status ON alert_history(notification_status);
-- 复合索引：分页查询优化（按时间倒序 + 筛选条件）
CREATE INDEX IF NOT EXISTS idx_alert_history_fired_severity ON alert_history(fired_at, severity);
CREATE INDEX IF NOT EXISTS idx_alert_history_project_fired ON alert_history(project_id, fired_at);

-- 告警冷却状态表（内存为主，DB 用于重启恢复）
CREATE TABLE alert_cooldowns (
    rule_id TEXT NOT NULL,
    scope_key TEXT NOT NULL,                  -- 冷却作用域（如 project_id 或 'global'）
    last_fired_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    PRIMARY KEY (rule_id, scope_key)
);

CREATE INDEX idx_alert_cooldowns_expires ON alert_cooldowns(expires_at);

-- system_configs 新增告警相关配置（system_configs 表在 V001 中已创建）
INSERT OR IGNORE INTO system_configs (key, value, description) VALUES
('alert_enabled', 'true', '全局告警开关'),
('alert_evaluation_interval_seconds', '30', '规则评估间隔（秒）'),
('alert_history_retention_days', '90', '告警历史保留天数');
```

### 5.2 Schema 关系图

```
┌──────────────────┐     ┌─────────────────────┐
│   alert_rules    │     │      projects       │
│──────────────────│     │─────────────────────│
│ rule_id (PK)     │     │ id (PK)             │
│ name             │     │ name                │
│ description      │     │ ...                 │
│ severity         │     └─────────────────────┘
│ enabled          │              │
│ threshold_json   │              │
│ cooldown_seconds │              │
│ updated_at       │              │
└──────────────────┘              │
        │                         │
        │ rule_id                 │ project_id
        ▼                         ▼
┌──────────────────────────────────────────┐
│            alert_history                  │
│──────────────────────────────────────────│
│ id (PK)                                  │
│ rule_id                                  │
│ severity                                 │
│ project_id (FK, nullable)                │
│ title                                    │
│ message                                  │
│ context_json                             │
│ fired_at                                 │
│ resolved_at                              │
│ notified_at                              │
│ notification_channel                     │
│ notification_status                      │
└──────────────────────────────────────────┘

┌──────────────────────────┐     ┌──────────────────────────┐
│  notification_channels   │     │    alert_cooldowns       │
│──────────────────────────│     │──────────────────────────│
│ channel_id (PK)          │     │ rule_id (PK)             │
│ name                     │     │ scope_key (PK)           │
│ channel_type             │     │ last_fired_at            │
│ enabled                  │     │ expires_at               │
│ config_encrypted         │     └──────────────────────────┘
│ severity_filter_json     │
│ last_test_at             │
│ last_test_success        │
│ updated_at               │
└──────────────────────────┘
```

---

## 6. 内部架构设计

### 6.1 架构总览

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Alert & Notification System                    │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌────────────────────────────────────────────────────────────┐     │
│  │                    AlertEngine                              │     │
│  │                                                            │     │
│  │  ┌──────────────────┐    ┌──────────────────────────┐     │     │
│  │  │ MetricCollector  │───►│    RuleEvaluator          │     │     │
│  │  │                  │    │                            │     │     │
│  │  │ - task_metrics   │    │ - evaluate_all()           │     │     │
│  │  │ - process_metrics│    │ - check_cooldown()         │     │     │
│  │  │ - api_metrics    │    │ - fire_alert()             │     │     │
│  │  │ - concurrency    │    └────────────┬───────────────┘     │     │
│  │  └──────────────────┘                 │ AlertEvent          │     │
│  └───────────────────────────────────────┼─────────────────────┘     │
│                                          │                           │
│                                          ▼                           │
│  ┌────────────────────────────────────────────────────────────┐     │
│  │              NotificationDispatcher                         │     │
│  │                                                            │     │
│  │  ┌──────────────────┐    ┌──────────────────────────┐     │     │
│  │  │  ChannelRouter   │───►│  NotificationChannel(s)   │     │     │
│  │  │                  │    │                            │     │     │
│  │  │ - severity_match │    │ ┌────────────────────────┐│     │     │
│  │  │ - channel_select │    │ │  DingTalkChannel       ││     │     │
│  │  │ - format_message │    │ ├────────────────────────┤│     │     │
│  │  └──────────────────┘    │ │  (future) SlackChannel ││     │     │
│  │                          │ ├────────────────────────┤│     │     │
│  │                          │ │  (future) EmailChannel ││     │     │
│  │                          │ └────────────────────────┘│     │     │
│  │                          └──────────────────────────┘     │     │
│  └────────────────────────────────────────────────────────────┘     │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────┐     │
│  │                    AlertHistoryStore                        │     │
│  │  - record_alert()                                          │     │
│  │  - update_notification_status()                            │     │
│  │  - query_history()                                         │     │
│  │  - cleanup_expired()                                       │     │
│  └────────────────────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────────────────────┘
```

### 6.2 核心 Trait 定义

#### MetricCollector

```rust
use std::collections::HashMap;
use chrono::{DateTime, Utc};

/// 指标数据点
#[derive(Debug, Clone)]
pub struct MetricSnapshot {
    /// 各项目的任务运行时间（project_id -> Vec<(agent_id, issue_iid, started_at)>）
    pub running_tasks: HashMap<i64, Vec<RunningTask>>,
    /// 各项目的服务状态（project_id -> ServiceHealthStatus）
    pub service_health: HashMap<i64, ServiceHealthStatus>,
    /// 全局并行状态
    pub concurrency: ConcurrencyMetrics,
    /// API 健康状态（platform -> consecutive_failures）
    pub api_health: HashMap<String, u64>,
    /// 采集时间
    pub collected_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RunningTask {
    pub agent_id: String,
    pub issue_iid: i64,
    pub issue_title: String,
    pub project_id: i64,
    pub started_at: DateTime<Utc>,
    pub elapsed_seconds: i64,
}

#[derive(Debug, Clone)]
pub enum ServiceHealthStatus {
    Running,
    Stopped,
    Crashed { exit_code: i32, crashed_at: DateTime<Utc> },
}

#[derive(Debug, Clone)]
pub struct ConcurrencyMetrics {
    pub global_max: i64,
    pub global_active: i64,
    pub saturated_since: Option<DateTime<Utc>>,
}

/// 指标采集器 — 从 ProcessManager 和 ConcurrencyManager 聚合运行时指标
#[async_trait]
pub trait MetricCollector: Send + Sync {
    /// 采集当前所有指标快照
    async fn collect(&self) -> Result<MetricSnapshot>;

    /// 记录任务失败事件（用于连续失败计数）
    async fn record_task_failure(&self, project_id: i64, agent_id: &str, issue_iid: i64);

    /// 记录任务成功事件（重置连续失败计数）
    async fn record_task_success(&self, project_id: i64);

    /// 记录 API 调用失败
    async fn record_api_failure(&self, platform: &str);

    /// 记录 API 调用成功（重置连续失败计数）
    async fn record_api_success(&self, platform: &str);

    /// 获取项目连续失败次数
    async fn get_consecutive_failures(&self, project_id: i64) -> u64;

    /// 获取平台 API 连续失败次数
    async fn get_api_consecutive_failures(&self, platform: &str) -> u64;

    /// 记录服务崩溃事件（进程意外退出）
    async fn record_service_crash(&self, project_id: i64, exit_code: i32);
}
```

#### RuleEvaluator

```rust
/// 告警事件（由 RuleEvaluator 产生，传递给 NotificationDispatcher）
#[derive(Debug, Clone, Serialize)]
pub struct AlertEvent {
    pub id: String,
    pub rule_id: String,
    pub severity: Severity,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
    pub title: String,
    pub message: String,
    pub context: HashMap<String, String>,
    pub fired_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Critical => write!(f, "critical"),
            Severity::Warning => write!(f, "warning"),
            Severity::Info => write!(f, "info"),
        }
    }
}

/// 规则评估器 — 根据指标快照和规则配置评估是否触发告警
#[async_trait]
pub trait RuleEvaluator: Send + Sync {
    /// 评估所有启用的规则，返回触发的告警事件列表
    async fn evaluate(&self, metrics: &MetricSnapshot) -> Vec<AlertEvent>;

    /// 检查指定规则在指定作用域是否处于冷却期
    async fn is_in_cooldown(&self, rule_id: &str, scope_key: &str) -> bool;

    /// 记录规则触发（更新冷却状态）
    async fn mark_fired(&self, rule_id: &str, scope_key: &str, cooldown_seconds: i64);

    /// 重新加载规则配置（当管理员修改规则后调用）
    async fn reload_rules(&self) -> Result<()>;
}
```

#### NotificationDispatcher & ChannelRouter

```rust
/// 通知发送结果
#[derive(Debug, Clone)]
pub struct NotificationResult {
    pub channel_id: String,
    pub channel_type: String,
    pub success: bool,
    pub error_message: Option<String>,
    pub response_time_ms: u64,
    pub sent_at: DateTime<Utc>,
}

/// 通知渠道 trait — 所有通知渠道必须实现此接口
#[async_trait]
pub trait NotificationChannel: Send + Sync {
    /// 渠道类型标识
    fn channel_type(&self) -> &str;

    /// 渠道 ID
    fn channel_id(&self) -> &str;

    /// 发送告警通知
    async fn send(&self, alert: &AlertEvent) -> Result<NotificationResult>;

    /// 发送测试通知
    async fn send_test(&self, message: &str, operator: &str) -> Result<NotificationResult>;

    /// 健康检查（验证配置有效性，不实际发送消息）
    async fn health_check(&self) -> bool;
}

/// 渠道路由器 — 根据告警严重级别和渠道配置决定发送目标
#[async_trait]
pub trait ChannelRouter: Send + Sync {
    /// 根据告警事件选择目标渠道
    async fn route(&self, alert: &AlertEvent) -> Vec<Arc<dyn NotificationChannel>>;

    /// 重新加载渠道配置
    async fn reload_channels(&self) -> Result<()>;

    /// 获取指定渠道（用于测试通知）
    async fn get_channel(&self, channel_id: &str) -> Option<Arc<dyn NotificationChannel>>;
}

/// 通知分发器 — 接收告警事件并通过路由器分发到各渠道
#[async_trait]
pub trait NotificationDispatcher: Send + Sync {
    /// 分发告警通知到所有匹配的渠道
    async fn dispatch(&self, alert: &AlertEvent) -> Vec<NotificationResult>;

    /// 发送测试通知到指定渠道
    async fn send_test(
        &self,
        channel_id: &str,
        message: &str,
        operator: &str,
    ) -> Result<NotificationResult>;
}
```

#### AlertEngine（顶层协调器）

```rust
use std::sync::Arc;
use tokio::sync::mpsc;

/// 告警引擎 — 顶层协调器，驱动指标采集→规则评估→通知分发的完整流程
pub struct AlertEngine {
    metric_collector: Arc<dyn MetricCollector>,
    rule_evaluator: Arc<dyn RuleEvaluator>,
    notification_dispatcher: Arc<dyn NotificationDispatcher>,
    alert_history_store: Arc<dyn AlertHistoryStore>,
    /// 评估间隔（默认 30 秒）
    evaluation_interval: std::time::Duration,
    /// 停止信号
    shutdown_rx: mpsc::Receiver<()>,
}

impl AlertEngine {
    pub fn new(
        metric_collector: Arc<dyn MetricCollector>,
        rule_evaluator: Arc<dyn RuleEvaluator>,
        notification_dispatcher: Arc<dyn NotificationDispatcher>,
        alert_history_store: Arc<dyn AlertHistoryStore>,
        evaluation_interval: std::time::Duration,
        shutdown_rx: mpsc::Receiver<()>,
    ) -> Self {
        Self {
            metric_collector,
            rule_evaluator,
            notification_dispatcher,
            alert_history_store,
            evaluation_interval,
            shutdown_rx,
        }
    }

    /// 启动告警引擎主循环
    pub async fn run(&mut self) {
        let mut interval = tokio::time::interval(self.evaluation_interval);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.evaluate_cycle().await;
                }
                _ = self.shutdown_rx.recv() => {
                    tracing::info!("AlertEngine shutting down");
                    break;
                }
            }
        }
    }

    /// 单次评估循环
    async fn evaluate_cycle(&self) {
        // 1. 采集指标
        let metrics = match self.metric_collector.collect().await {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("Failed to collect metrics: {}", e);
                return;
            }
        };

        // 2. 评估规则
        let alerts = self.rule_evaluator.evaluate(&metrics).await;

        // 3. 对每个告警事件：记录历史 + 分发通知
        for alert in alerts {
            // 记录到历史
            let history_id = self.alert_history_store
                .record_alert(&alert)
                .await
                .unwrap_or_default();

            // 分发通知
            let results = self.notification_dispatcher.dispatch(&alert).await;

            // 更新通知状态
            for result in &results {
                let status = if result.success { "sent" } else { "failed" };
                let _ = self.alert_history_store
                    .update_notification_status(
                        history_id,
                        &result.channel_id,
                        status,
                        result.sent_at,
                    )
                    .await;
            }
        }
    }
}
```

#### AlertHistoryStore

```rust
/// 告警历史存储 trait
#[async_trait]
pub trait AlertHistoryStore: Send + Sync {
    /// 记录告警事件，返回记录 ID
    async fn record_alert(&self, alert: &AlertEvent) -> Result<i64>;

    /// 更新通知发送状态
    async fn update_notification_status(
        &self,
        alert_id: i64,
        channel_id: &str,
        status: &str,
        notified_at: DateTime<Utc>,
    ) -> Result<()>;

    /// 查询告警历史（分页 + 筛选）
    async fn query_history(&self, query: &AlertHistoryQuery) -> Result<(Vec<AlertHistoryRecord>, i64)>;

    /// 清理过期历史记录
    async fn cleanup_expired(&self, retention_days: i64) -> Result<u64>;
}
```

---

### 6.3 DingTalk Channel 实现

```rust
use hmac::{Hmac, Mac};
use sha2::Sha256;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

/// 钉钉群机器人通知渠道
pub struct DingTalkChannel {
    channel_id: String,
    webhook_url: String,
    secret: Option<String>,
    http_client: reqwest::Client,
}

impl DingTalkChannel {
    pub fn new(
        channel_id: String,
        webhook_url: String,
        secret: Option<String>,
    ) -> Self {
        Self {
            channel_id,
            webhook_url,
            secret,
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap(),
        }
    }

    /// 生成钉钉签名
    fn sign(&self, timestamp: i64) -> Option<String> {
        let secret = self.secret.as_ref()?;
        let string_to_sign = format!("{}\n{}", timestamp, secret);

        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).ok()?;
        mac.update(string_to_sign.as_bytes());
        let result = mac.finalize();

        Some(BASE64.encode(result.into_bytes()))
    }

    /// 构建带签名的请求 URL
    fn build_url(&self) -> String {
        if self.secret.is_some() {
            let timestamp = chrono::Utc::now().timestamp_millis();
            let sign = self.sign(timestamp).unwrap_or_default();
            format!(
                "{}&timestamp={}&sign={}",
                self.webhook_url, timestamp, urlencoding::encode(&sign)
            )
        } else {
            self.webhook_url.clone()
        }
    }

    /// 格式化告警为钉钉 Markdown 消息
    fn format_alert_message(&self, alert: &AlertEvent) -> serde_json::Value {
        let severity_icon = match alert.severity {
            Severity::Critical => "🔴",
            Severity::Warning => "🟡",
            Severity::Info => "🔵",
        };

        let project_info = alert.project_name.as_deref().unwrap_or("系统");
        let time_str = alert.fired_at.format("%Y-%m-%d %H:%M:%S").to_string();

        let mut text = format!(
            "### {} {}\n\n**项目**: {}\n\n**详情**: {}\n\n**时间**: {}",
            severity_icon, alert.title, project_info, alert.message, time_str
        );

        // 附加上下文信息
        if !alert.context.is_empty() {
            text.push_str("\n\n**上下文**:\n");
            for (key, value) in &alert.context {
                text.push_str(&format!("- {}: {}\n", key, value));
            }
        }

        serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "title": format!("{} {}", severity_icon, alert.title),
                "text": text
            }
        })
    }
}

#[async_trait]
impl NotificationChannel for DingTalkChannel {
    fn channel_type(&self) -> &str {
        "dingtalk"
    }

    fn channel_id(&self) -> &str {
        &self.channel_id
    }

    async fn send(&self, alert: &AlertEvent) -> Result<NotificationResult> {
        let url = self.build_url();
        let body = self.format_alert_message(alert);
        let start = std::time::Instant::now();

        let response = self.http_client
            .post(&url)
            .json(&body)
            .send()
            .await?;

        let elapsed = start.elapsed().as_millis() as u64;
        let status = response.status();
        let response_body: serde_json::Value = response.json().await?;

        let success = status.is_success()
            && response_body.get("errcode").and_then(|v| v.as_i64()) == Some(0);

        let error_message = if !success {
            Some(format!(
                "HTTP {}: {}",
                status,
                response_body.get("errmsg").and_then(|v| v.as_str()).unwrap_or("unknown error")
            ))
        } else {
            None
        };

        Ok(NotificationResult {
            channel_id: self.channel_id.clone(),
            channel_type: "dingtalk".to_string(),
            success,
            error_message,
            response_time_ms: elapsed,
            sent_at: Utc::now(),
        })
    }

    async fn send_test(&self, message: &str, operator: &str) -> Result<NotificationResult> {
        let url = self.build_url();
        let time_str = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let body = serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "title": "Symphony 通知测试",
                "text": format!(
                    "### Symphony 通知测试\n\n**状态**: 连通性验证成功\n\n**渠道**: 钉钉群机器人\n\n**时间**: {}\n\n**操作人**: {}\n\n---\n\n{}",
                    time_str, operator, message
                )
            }
        });

        let start = std::time::Instant::now();
        let response = self.http_client
            .post(&url)
            .json(&body)
            .send()
            .await?;

        let elapsed = start.elapsed().as_millis() as u64;
        let status = response.status();
        let response_body: serde_json::Value = response.json().await?;

        let success = status.is_success()
            && response_body.get("errcode").and_then(|v| v.as_i64()) == Some(0);

        let error_message = if !success {
            Some(format!(
                "HTTP {}: {}",
                status,
                response_body.get("errmsg").and_then(|v| v.as_str()).unwrap_or("unknown error")
            ))
        } else {
            None
        };

        Ok(NotificationResult {
            channel_id: self.channel_id.clone(),
            channel_type: "dingtalk".to_string(),
            success,
            error_message,
            response_time_ms: elapsed,
            sent_at: Utc::now(),
        })
    }

    async fn health_check(&self) -> bool {
        // 钉钉没有专门的健康检查 API，验证 URL 格式即可
        self.webhook_url.starts_with("https://oapi.dingtalk.com/robot/send")
    }
}
```

---

### 6.4 冷却/防抖机制

```rust
use dashmap::DashMap;
use chrono::{DateTime, Utc, Duration};

/// 内存中的冷却状态管理器
pub struct CooldownManager {
    /// (rule_id, scope_key) -> expires_at
    cooldowns: DashMap<(String, String), DateTime<Utc>>,
}

impl CooldownManager {
    pub fn new() -> Self {
        Self {
            cooldowns: DashMap::new(),
        }
    }

    /// 检查是否在冷却期内
    pub fn is_cooling_down(&self, rule_id: &str, scope_key: &str) -> bool {
        let key = (rule_id.to_string(), scope_key.to_string());
        if let Some(expires_at) = self.cooldowns.get(&key) {
            return Utc::now() < *expires_at;
        }
        false
    }

    /// 标记规则已触发，进入冷却期
    pub fn mark_fired(&self, rule_id: &str, scope_key: &str, cooldown_seconds: i64) {
        let key = (rule_id.to_string(), scope_key.to_string());
        let expires_at = Utc::now() + Duration::seconds(cooldown_seconds);
        self.cooldowns.insert(key, expires_at);
    }

    /// 清理已过期的冷却记录（定期调用）
    pub fn cleanup_expired(&self) {
        let now = Utc::now();
        self.cooldowns.retain(|_, expires_at| *expires_at > now);
    }

    /// 从数据库恢复冷却状态（启动时调用）
    pub fn restore_from_db(&self, records: Vec<(String, String, DateTime<Utc>)>) {
        let now = Utc::now();
        for (rule_id, scope_key, expires_at) in records {
            if expires_at > now {
                self.cooldowns.insert((rule_id, scope_key), expires_at);
            }
        }
    }
}
```

**冷却作用域（scope_key）规则**:

| 规则 | scope_key | 说明 |
|------|-----------|------|
| task_timeout | `project:{project_id}:issue:{issue_iid}` | 同一任务不重复告警 |
| task_failure | `project:{project_id}:issue:{issue_iid}` | 同一任务不重复告警 |
| service_crash | `project:{project_id}` | 同一项目不重复告警 |
| concurrency_saturation | `global` | 全局唯一 |
| consecutive_failures | `project:{project_id}` | 同一项目不重复告警 |
| api_unreachable | `platform:{platform}` | 同一平台不重复告警 |

---

### 6.5 规则评估逻辑

```rust
/// 默认规则评估器实现
pub struct DefaultRuleEvaluator {
    rules: Arc<tokio::sync::RwLock<Vec<AlertRule>>>,
    cooldown_manager: Arc<CooldownManager>,
    metric_collector: Arc<dyn MetricCollector>,
}

impl DefaultRuleEvaluator {
    /// 评估 task_timeout 规则
    async fn evaluate_task_timeout(
        &self,
        rule: &AlertRule,
        metrics: &MetricSnapshot,
    ) -> Vec<AlertEvent> {
        let timeout_minutes = rule.threshold
            .get("timeout_minutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);

        let mut alerts = Vec::new();

        for (project_id, tasks) in &metrics.running_tasks {
            for task in tasks {
                let elapsed_minutes = task.elapsed_seconds / 60;
                if elapsed_minutes >= timeout_minutes as i64 {
                    let scope_key = format!("project:{}:issue:{}", project_id, task.issue_iid);

                    if !self.cooldown_manager.is_cooling_down(&rule.rule_id, &scope_key) {
                        alerts.push(AlertEvent {
                            id: uuid::Uuid::new_v4().to_string(),
                            rule_id: rule.rule_id.clone(),
                            severity: Severity::Warning,
                            project_id: Some(*project_id),
                            project_name: None, // 由调用方填充
                            title: "任务超时告警".to_string(),
                            message: format!(
                                "Issue #{} ({}) 运行时间已超过 {} 分钟（当前 {} 分钟）",
                                task.issue_iid, task.issue_title,
                                timeout_minutes, elapsed_minutes
                            ),
                            context: HashMap::from([
                                ("issue_iid".to_string(), task.issue_iid.to_string()),
                                ("issue_title".to_string(), task.issue_title.clone()),
                                ("duration_minutes".to_string(), elapsed_minutes.to_string()),
                                ("threshold_minutes".to_string(), timeout_minutes.to_string()),
                                ("agent_id".to_string(), task.agent_id.clone()),
                            ]),
                            fired_at: Utc::now(),
                        });

                        self.cooldown_manager.mark_fired(
                            &rule.rule_id,
                            &scope_key,
                            rule.cooldown_seconds,
                        );
                    }
                }
            }
        }

        alerts
    }

    /// 评估 service_crash 规则
    async fn evaluate_service_crash(
        &self,
        rule: &AlertRule,
        metrics: &MetricSnapshot,
    ) -> Vec<AlertEvent> {
        let mut alerts = Vec::new();

        for (project_id, health) in &metrics.service_health {
            if let ServiceHealthStatus::Crashed { exit_code, crashed_at } = health {
                let scope_key = format!("project:{}", project_id);

                if !self.cooldown_manager.is_cooling_down(&rule.rule_id, &scope_key) {
                    alerts.push(AlertEvent {
                        id: uuid::Uuid::new_v4().to_string(),
                        rule_id: rule.rule_id.clone(),
                        severity: Severity::Critical,
                        project_id: Some(*project_id),
                        project_name: None,
                        title: "服务异常退出".to_string(),
                        message: format!(
                            "Symphony 实例进程意外退出 (exit code: {})",
                            exit_code
                        ),
                        context: HashMap::from([
                            ("exit_code".to_string(), exit_code.to_string()),
                            ("crashed_at".to_string(), crashed_at.to_rfc3339()),
                        ]),
                        fired_at: Utc::now(),
                    });

                    self.cooldown_manager.mark_fired(
                        &rule.rule_id,
                        &scope_key,
                        rule.cooldown_seconds,
                    );
                }
            }
        }

        alerts
    }

    /// 评估 concurrency_saturation 规则
    async fn evaluate_concurrency_saturation(
        &self,
        rule: &AlertRule,
        metrics: &MetricSnapshot,
    ) -> Vec<AlertEvent> {
        let saturation_minutes = rule.threshold
            .get("saturation_minutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(10);

        let mut alerts = Vec::new();

        if metrics.concurrency.global_active >= metrics.concurrency.global_max {
            if let Some(saturated_since) = metrics.concurrency.saturated_since {
                let saturated_minutes = (Utc::now() - saturated_since).num_minutes();
                if saturated_minutes >= saturation_minutes as i64 {
                    let scope_key = "global".to_string();

                    if !self.cooldown_manager.is_cooling_down(&rule.rule_id, &scope_key) {
                        alerts.push(AlertEvent {
                            id: uuid::Uuid::new_v4().to_string(),
                            rule_id: rule.rule_id.clone(),
                            severity: Severity::Warning,
                            project_id: None,
                            project_name: None,
                            title: "并行饱和告警".to_string(),
                            message: format!(
                                "全局并行数已达上限 ({}/{}) 持续 {} 分钟",
                                metrics.concurrency.global_active,
                                metrics.concurrency.global_max,
                                saturated_minutes
                            ),
                            context: HashMap::from([
                                ("global_active".to_string(), metrics.concurrency.global_active.to_string()),
                                ("global_max".to_string(), metrics.concurrency.global_max.to_string()),
                                ("saturated_minutes".to_string(), saturated_minutes.to_string()),
                                ("threshold_minutes".to_string(), saturation_minutes.to_string()),
                            ]),
                            fired_at: Utc::now(),
                        });

                        self.cooldown_manager.mark_fired(
                            &rule.rule_id,
                            &scope_key,
                            rule.cooldown_seconds,
                        );
                    }
                }
            }
        }

        alerts
    }

    /// 评估 consecutive_failures 规则
    async fn evaluate_consecutive_failures(
        &self,
        rule: &AlertRule,
        metrics: &MetricSnapshot,
    ) -> Vec<AlertEvent> {
        let failure_count_threshold = rule.threshold
            .get("failure_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(3);

        let mut alerts = Vec::new();

        for project_id in metrics.running_tasks.keys() {
            let failures = self.metric_collector
                .get_consecutive_failures(*project_id)
                .await;

            if failures >= failure_count_threshold {
                let scope_key = format!("project:{}", project_id);

                if !self.cooldown_manager.is_cooling_down(&rule.rule_id, &scope_key) {
                    alerts.push(AlertEvent {
                        id: uuid::Uuid::new_v4().to_string(),
                        rule_id: rule.rule_id.clone(),
                        severity: Severity::Critical,
                        project_id: Some(*project_id),
                        project_name: None,
                        title: "连续失败告警".to_string(),
                        message: format!(
                            "项目连续 {} 个任务失败（阈值 {}）",
                            failures, failure_count_threshold
                        ),
                        context: HashMap::from([
                            ("consecutive_failures".to_string(), failures.to_string()),
                            ("threshold".to_string(), failure_count_threshold.to_string()),
                        ]),
                        fired_at: Utc::now(),
                    });

                    self.cooldown_manager.mark_fired(
                        &rule.rule_id,
                        &scope_key,
                        rule.cooldown_seconds,
                    );
                }
            }
        }

        alerts
    }

    /// 评估 api_unreachable 规则
    async fn evaluate_api_unreachable(
        &self,
        rule: &AlertRule,
        metrics: &MetricSnapshot,
    ) -> Vec<AlertEvent> {
        let failure_count_threshold = rule.threshold
            .get("failure_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(5);

        let mut alerts = Vec::new();

        for (platform, consecutive_failures) in &metrics.api_health {
            if *consecutive_failures >= failure_count_threshold {
                let scope_key = format!("platform:{}", platform);

                if !self.cooldown_manager.is_cooling_down(&rule.rule_id, &scope_key) {
                    alerts.push(AlertEvent {
                        id: uuid::Uuid::new_v4().to_string(),
                        rule_id: rule.rule_id.clone(),
                        severity: Severity::Critical,
                        project_id: None,
                        project_name: None,
                        title: "API 不可达告警".to_string(),
                        message: format!(
                            "{} API 连续 {} 次请求失败（阈值 {}）",
                            platform, consecutive_failures, failure_count_threshold
                        ),
                        context: HashMap::from([
                            ("platform".to_string(), platform.clone()),
                            ("consecutive_failures".to_string(), consecutive_failures.to_string()),
                            ("threshold".to_string(), failure_count_threshold.to_string()),
                        ]),
                        fired_at: Utc::now(),
                    });

                    self.cooldown_manager.mark_fired(
                        &rule.rule_id,
                        &scope_key,
                        rule.cooldown_seconds,
                    );
                }
            }
        }

        alerts
    }
}
```

---

## 7. AppState 扩展

### 7.1 新增 AlertManager

```rust
use std::sync::Arc;
use tokio::sync::mpsc;

/// 告警管理器 — 封装告警系统的所有组件，挂载到 AppState
#[derive(Clone)]
pub struct AlertManager {
    /// 告警引擎停止信号发送端
    shutdown_tx: mpsc::Sender<()>,
    /// 冷却管理器（供 API handler 查询冷却状态）
    pub cooldown_manager: Arc<CooldownManager>,
    /// 规则评估器（供 API handler 触发规则重载）
    pub rule_evaluator: Arc<dyn RuleEvaluator>,
    /// 通知分发器（供测试通知 API 使用）
    pub notification_dispatcher: Arc<dyn NotificationDispatcher>,
    /// 渠道路由器（供配置变更后重载）
    pub channel_router: Arc<dyn ChannelRouter>,
    /// 指标采集器（供外部事件注入）
    pub metric_collector: Arc<dyn MetricCollector>,
}
```

### 7.2 更新 AppState

```rust
#[derive(Clone)]
pub struct AppState {
    pub repo: SqliteRepository,
    pub jwt_secret: String,
    pub encryption_key: [u8; 32],
    pub token_blacklist: Arc<DashMap<i64, chrono::DateTime<Utc>>>,
    pub rate_limiter: Arc<RateLimiter>,
    pub process_manager: ProcessManager,
    pub api_cache: Arc<ApiCache>,
    pub ai_service: Option<Arc<AiService>>,
    pub phase3_rate_limiter: Arc<Phase3RateLimiter>,
    pub concurrency_manager: ConcurrencyManager,
    // Phase 5 additions:
    pub alert_manager: AlertManager,  // 新增
}
```

---

## 8. 新增 Repository Trait 方法

### 8.1 AlertRepository

```rust
#[async_trait]
pub trait AlertRepository: Send + Sync {
    // --- Alert Rules ---

    /// 获取所有告警规则
    async fn get_all_alert_rules(&self) -> Result<Vec<AlertRule>>;

    /// 获取指定告警规则
    async fn get_alert_rule(&self, rule_id: &str) -> Result<Option<AlertRule>>;

    /// 更新告警规则
    async fn update_alert_rule(
        &self,
        rule_id: &str,
        enabled: Option<bool>,
        threshold_json: Option<&str>,
        cooldown_seconds: Option<i64>,
    ) -> Result<()>;

    // --- Notification Channels ---

    /// 获取所有通知渠道配置
    async fn get_all_notification_channels(&self) -> Result<Vec<NotificationChannelRow>>;

    /// 获取指定通知渠道
    async fn get_notification_channel(&self, channel_id: &str) -> Result<Option<NotificationChannelRow>>;

    /// 保存通知渠道配置（全量替换）
    async fn save_notification_channels(&self, channels: Vec<NotificationChannelRow>) -> Result<()>;

    /// 更新渠道测试结果
    async fn update_channel_test_result(
        &self,
        channel_id: &str,
        success: bool,
        tested_at: &str,
    ) -> Result<()>;

    // --- Alert History ---

    /// 记录告警历史
    async fn insert_alert_history(&self, record: &InsertAlertHistory) -> Result<i64>;

    /// 更新告警通知状态
    async fn update_alert_notification_status(
        &self,
        id: i64,
        channel: &str,
        status: &str,
        notified_at: &str,
    ) -> Result<()>;

    /// 查询告警历史（分页 + 筛选）
    async fn query_alert_history(
        &self,
        query: &AlertHistoryQuery,
    ) -> Result<(Vec<AlertHistoryRecord>, i64)>;

    /// 清理过期告警历史
    async fn cleanup_alert_history(&self, retention_days: i64) -> Result<u64>;

    // --- Cooldown Persistence ---

    /// 保存冷却状态到数据库
    async fn save_cooldown(
        &self,
        rule_id: &str,
        scope_key: &str,
        last_fired_at: &str,
        expires_at: &str,
    ) -> Result<()>;

    /// 加载所有未过期的冷却状态
    async fn load_active_cooldowns(&self) -> Result<Vec<(String, String, String)>>;

    /// 清理已过期的冷却记录
    async fn cleanup_expired_cooldowns(&self) -> Result<u64>;
}

/// 数据库行模型（通知渠道）
#[derive(Debug, Clone)]
pub struct NotificationChannelRow {
    pub channel_id: String,
    pub name: String,
    pub channel_type: String,
    pub enabled: bool,
    pub config_encrypted: String,
    pub severity_filter_json: String,
    pub last_test_at: Option<String>,
    pub last_test_success: Option<bool>,
    pub created_at: String,
    pub updated_at: String,
}

/// 插入告警历史的参数
#[derive(Debug, Clone)]
pub struct InsertAlertHistory {
    pub rule_id: String,
    pub severity: String,
    pub project_id: Option<i64>,
    pub title: String,
    pub message: String,
    pub context_json: Option<String>,
    pub fired_at: String,
}
```

---

## 9. 新增路由注册

```rust
// In router.rs - add to admin_routes:
let admin_routes = Router::new()
    // ... existing admin routes ...
    // Phase 5: Alert & Notification
    .route("/api/admin/alerts", get(alerts::list_alert_history))
    .route("/api/admin/alerts/rules", get(alerts::get_alert_rules))
    .route("/api/admin/alerts/rules", put(alerts::update_alert_rules))
    .route("/api/admin/alerts/channels", get(alerts::get_alert_channels))
    .route("/api/admin/alerts/channels", put(alerts::update_alert_channels))
    .route("/api/admin/alerts/test", post(alerts::test_notification))
    .layer(middleware::from_fn(require_admin))
    .layer(middleware::from_fn_with_state(state.clone(), jwt_auth));
```

---

## 10. 安全考虑

### 10.1 敏感配置保护

- 通知渠道配置（webhook_url, secret）使用 AES-256-GCM 加密存储，复用现有 `encryption_key`
- API 响应中敏感字段脱敏显示（保留部分字符 + 掩码）
- 渠道配置变更记录审计日志（who, when, what changed）

### 10.2 防滥用

- 测试通知端点限速 3 次/分钟/用户，防止被用于 DDoS 钉钉 API
- 告警冷却机制防止同一告警短时间内重复发送
- 钉钉 Webhook URL 白名单校验（必须以 `https://oapi.dingtalk.com/robot/send` 开头）

### 10.3 告警引擎安全

- 告警引擎运行在独立 tokio task 中，panic 不影响主服务
- 通知发送超时 10 秒，防止慢响应阻塞评估循环
- 通知发送失败不影响告警记录（先记录历史，再尝试通知）

---

## 11. 性能考虑

### 11.1 评估循环优化

| 操作 | 频率 | 耗时预期 | 说明 |
|------|------|----------|------|
| 指标采集 | 30s | < 5ms | 从内存中的 ConcurrencyManager 读取 |
| 规则评估 | 30s | < 1ms | 纯内存计算 + 冷却检查 |
| 告警记录 | 按需 | < 10ms | SQLite 单行插入 |
| 通知发送 | 按需 | < 10s | 异步 HTTP 请求，不阻塞评估循环 |

### 11.2 数据库查询优化

- `alert_history` 表使用复合索引 `(fired_at, severity)` 加速分页查询
- `alert_history` 表使用索引 `(project_id, fired_at)` 加速按项目筛选
- 历史数据定期清理（默认保留 90 天）
- 冷却状态以内存为主，DB 仅用于重启恢复

### 11.3 通知发送优化

- 通知发送使用独立 tokio task，不阻塞评估循环
- 批量告警时并行发送到多个渠道
- 钉钉 API 限流：同一机器人每分钟最多 20 条消息，引擎侧做聚合

---

## 12. 完整端点汇总

### Phase 5 新增端点

| 方法 | 路径 | 权限 | 说明 |
|------|------|------|------|
| GET | /api/admin/alerts | Admin | 告警历史列表（分页、筛选） |
| GET | /api/admin/alerts/rules | Admin | 获取告警规则配置 |
| PUT | /api/admin/alerts/rules | Admin | 更新告警规则（启用/禁用、阈值） |
| GET | /api/admin/alerts/channels | Admin | 获取通知渠道配置 |
| PUT | /api/admin/alerts/channels | Admin | 更新通知渠道（钉钉 Webhook 等） |
| POST | /api/admin/alerts/test | Admin | 发送测试通知（验证渠道连通性） |

### 内部组件交互

```
ProcessManager ──────┐
                     │ metrics
ConcurrencyManager ──┼──► MetricCollector ──► RuleEvaluator ──► AlertEvent
                     │                                              │
API Error Counter ───┘                                              │
                                                                    ▼
                                                        NotificationDispatcher
                                                                    │
                                                            ChannelRouter
                                                                    │
                                                    ┌───────────────┼───────────────┐
                                                    ▼               ▼               ▼
                                              DingTalkChannel  (SlackChannel)  (EmailChannel)
```

---

## 13. 启动与关闭流程

### 13.1 启动流程

```rust
// In main.rs or lib.rs initialization:

// 1. 从数据库加载告警规则
let rules = repo.get_all_alert_rules().await?;

// 2. 从数据库加载通知渠道配置
let channels = repo.get_all_notification_channels().await?;

// 3. 初始化冷却管理器并恢复状态
let cooldown_manager = Arc::new(CooldownManager::new());
let active_cooldowns = repo.load_active_cooldowns().await?;
cooldown_manager.restore_from_db(
    active_cooldowns.into_iter().filter_map(|(rule_id, scope_key, expires_at)| {
        match expires_at.parse::<DateTime<Utc>>() {
            Ok(dt) => Some((rule_id, scope_key, dt)),
            Err(e) => {
                tracing::warn!("Skipping unparseable cooldown entry ({}, {}): {}", rule_id, scope_key, e);
                None
            }
        }
    }).collect()
);

// 4. 构建通知渠道实例
let channel_instances = build_channels(&channels, &encryption_key)?;

// 5. 构建各组件
let metric_collector = Arc::new(DefaultMetricCollector::new(
    process_manager.clone(),
    concurrency_manager.clone(),
));
let channel_router = Arc::new(DefaultChannelRouter::new(channel_instances));
let notification_dispatcher = Arc::new(DefaultNotificationDispatcher::new(
    channel_router.clone(),
));
let rule_evaluator = Arc::new(DefaultRuleEvaluator::new(
    rules,
    cooldown_manager.clone(),
    metric_collector.clone(),
));

// 6. 启动告警引擎
let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
let alert_history_store = Arc::new(SqliteAlertHistoryStore::new(repo.clone()));

let mut engine = AlertEngine::new(
    metric_collector.clone(),
    rule_evaluator.clone(),
    notification_dispatcher.clone(),
    alert_history_store,
    Duration::from_secs(30), // evaluation_interval
    shutdown_rx,
);

tokio::spawn(async move {
    engine.run().await;
});

// 7. 构建 AlertManager 并挂载到 AppState
let alert_manager = AlertManager {
    shutdown_tx,
    cooldown_manager,
    rule_evaluator,
    notification_dispatcher,
    channel_router,
    metric_collector,
};
```

### 13.2 优雅关闭

```rust
// In graceful shutdown handler:
// 发送停止信号给告警引擎
let _ = alert_manager.shutdown_tx.send(()).await;

// 持久化当前冷却状态到数据库
for entry in alert_manager.cooldown_manager.cooldowns.iter() {
    let (rule_id, scope_key) = entry.key();
    let expires_at = entry.value();
    let _ = repo.save_cooldown(
        rule_id,
        scope_key,
        &Utc::now().to_rfc3339(),
        &expires_at.to_rfc3339(),
    ).await;
}
```

---

## 14. 配置变更热重载

当管理员通过 API 修改规则或渠道配置时，需要通知运行中的告警引擎重新加载配置。

### 14.1 规则变更重载

```rust
// In alerts::update_alert_rules handler:
async fn update_alert_rules(
    State(state): State<AppState>,
    Json(req): Json<UpdateAlertRulesRequest>,
) -> impl IntoResponse {
    // 1. 校验并更新数据库
    // ...

    // 2. 通知规则评估器重载
    state.alert_manager.rule_evaluator.reload_rules().await?;

    // 3. 返回更新后的规则
    // ...
}
```

### 14.2 渠道变更重载

```rust
// In alerts::update_alert_channels handler:
async fn update_alert_channels(
    State(state): State<AppState>,
    Json(req): Json<UpdateAlertChannelsRequest>,
) -> impl IntoResponse {
    // 1. 校验并更新数据库
    // ...

    // 2. 通知渠道路由器重载
    state.alert_manager.channel_router.reload_channels().await?;

    // 3. 返回更新后的渠道配置
    // ...
}
```

---

## 15. 与 ProcessManager 集成

### 15.1 服务崩溃事件注入

在现有 ProcessManager 的 watcher 中，当检测到进程异常退出时，注入指标事件：

```rust
// In process_manager/watcher.rs - existing process monitoring loop:

// 当检测到进程异常退出时：
if let Some(exit_status) = child.try_wait()? {
    if !exit_status.success() {
        // 通知指标采集器记录服务崩溃
        alert_manager.metric_collector.record_service_crash(
            project_id,
            exit_status.code().unwrap_or(-1),
        ).await;
    }
}
```

### 15.2 任务完成事件注入

当 ConcurrencyManager 检测到 Agent 完成任务时：

```rust
// In concurrency watcher - when agent_completed event is detected:

// 判断任务是否成功
if task_succeeded {
    alert_manager.metric_collector.record_task_success(project_id).await;
} else {
    alert_manager.metric_collector.record_task_failure(
        project_id,
        &agent_id,
        issue_iid,
    ).await;
}
```

### 15.3 API 调用失败事件注入

在 GitPlatformClient 的调用层，记录 API 失败：

```rust
// In services/git_platform.rs - API call wrapper:

match self.client.list_issues(token, project_path, &options).await {
    Ok(issues) => {
        alert_manager.metric_collector.record_api_success(&platform).await;
        Ok(issues)
    }
    Err(e) => {
        alert_manager.metric_collector.record_api_failure(&platform).await;
        Err(e)
    }
}
```
