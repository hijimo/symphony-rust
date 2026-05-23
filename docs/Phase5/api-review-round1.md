# Phase 5 API Design Review — Round 1

## Verdict: PASS WITH ISSUES

The document is well-structured, comprehensive, and closely follows the patterns established in Phase 4. The architecture is sound, the DingTalk signing algorithm is correct, and all 6 alert rules from section 10.2 are covered. However, there are several consistency issues with the existing codebase that must be fixed before implementation.

---

## Critical Issues (Must Fix)

### C1. Pagination type mismatch — should reuse existing `PaginationData<T>`

**Section**: 1.1 GET /api/admin/alerts — Rust Data Model

**Problem**: The document defines a custom `AlertHistoryListResponse` struct with `u32` and `u64` types for pagination fields:

```rust
pub struct AlertHistoryListResponse {
    pub limit: u32,
    pub offset: u32,
    pub page_no: u32,
    pub page_size: u32,
    pub pages: u32,
    pub records: Vec<AlertHistoryRecord>,
    pub total_count: u64,
}
```

The existing codebase uses a generic `PaginationData<T>` struct (in `web-platform/src/models/response.rs`) with `i64` for all numeric fields:

```rust
pub struct PaginationData<T: Serialize> {
    pub limit: i64,
    pub offset: i64,
    pub page_no: i64,
    pub page_size: i64,
    pub pages: i64,
    pub records: Vec<T>,
    pub total_count: i64,
}
```

All existing paginated endpoints (`admin_users.rs`, `projects.rs`) use `ResponseData<PaginationData<T>>`.

**Fix**: Remove `AlertHistoryListResponse` and use `PaginationData<AlertHistoryRecord>` instead. Change `AlertHistoryQuery` page fields from `Option<u32>` to `Option<i64>` to match existing patterns.

---

### C2. Query parameter naming inconsistency — snake_case vs camelCase

**Section**: 1.1 GET /api/admin/alerts — Query Parameters

**Problem**: The query parameters use `snake_case` (`rule_id`, `project_id`, `start_time`, `end_time`), but the struct has `#[serde(rename_all = "camelCase")]`. With `camelCase` rename, the actual query parameters accepted would be `ruleId`, `projectId`, `startTime`, `endTime`.

Looking at the existing codebase, `KanbanQuery` in Phase 3 uses `snake_case` field names without `rename_all` for query deserialization (e.g., `todo_limit`, `no_cache`). The `ProjectListQuery` also uses snake_case fields.

**Fix**: Remove `#[serde(rename_all = "camelCase")]` from `AlertHistoryQuery` since it's a query parameter struct (deserialized from URL params, not JSON body). Keep snake_case field names as documented in the query parameter table. Only JSON response bodies should use camelCase.

---

### C3. JSON response field naming inconsistency in example

**Section**: 1.1 GET /api/admin/alerts — Success Response JSON

**Problem**: The JSON response example uses `snake_case` for fields like `rule_id`, `project_id`, `project_name`, `fired_at`, `resolved_at`, `notified_at`, `notification_channel`, `notification_status`. However, the struct has `#[serde(rename_all = "camelCase")]`, which means the actual JSON output would be `ruleId`, `projectId`, `projectName`, `firedAt`, etc.

The existing codebase convention is clear: response structs use `#[serde(rename_all = "camelCase")]` and JSON output is camelCase. The existing `PaginationData` uses explicit `#[serde(rename = "pageNo")]` etc.

**Fix**: Update all JSON response examples in sections 1.1, 2.1, 2.2, 3.1, 3.2, 4.1 to use camelCase field names (e.g., `ruleId`, `projectId`, `firedAt`, `notificationStatus`). The Rust structs are correct with `rename_all = "camelCase"`, but the examples must match.

---

### C4. MetricCollector trait missing `record_service_crash` method

**Section**: 6.2 MetricCollector trait / 15.1 Integration with ProcessManager

**Problem**: Section 15.1 calls `alert_manager.metric_collector.record_service_crash(project_id, exit_code)`, but this method is not defined in the `MetricCollector` trait in section 6.2. The trait only defines:
- `collect()`
- `record_task_failure()`
- `record_task_success()`
- `record_api_failure()`
- `record_api_success()`
- `get_consecutive_failures()`
- `get_api_consecutive_failures()`

**Fix**: Add `record_service_crash` to the `MetricCollector` trait:

```rust
/// Record a service crash event (process unexpected exit)
async fn record_service_crash(&self, project_id: i64, exit_code: i32);
```

---

### C5. `issue_iid` type inconsistency — `u64` vs `i64`

**Section**: 6.2 MetricCollector, 6.5 Rule Evaluator

**Problem**: The `RunningTask` struct uses `pub issue_iid: u64`, and `record_task_failure` takes `issue_iid: u64`. However, the existing `ConcurrencyRepository` trait in `traits.rs` uses `issue_iid: Option<i64>`:

```rust
async fn record_concurrency_event(
    &self,
    ...
    issue_iid: Option<i64>,
    ...
) -> Result<()>;
```

The existing `ConcurrencyEvent` model also uses `Option<i64>` for `issue_iid`. SQLite stores integers as signed, so `i64` is the correct type for database interop.

**Fix**: Change all `issue_iid: u64` occurrences in the Phase 5 design to `issue_iid: i64` (or `Option<i64>` where nullable) to match existing patterns.

---

### C6. New error codes not mapped to `WebPlatformError` variants

**Section**: Common Protocol — New Error Codes

**Problem**: The document introduces `ALERT_001`, `ALERT_002`, `ALERT_003` error codes but doesn't specify how they map to `WebPlatformError` enum variants. The existing error system uses enum variants that map to specific codes in `IntoResponse`:

- `NotFound(msg)` -> `BIZ_002` (404)
- `BadRequest(msg)` -> `BIZ_001` (400)
- `ExternalService(msg)` -> `EXT_001` (502)

The new codes need new enum variants or the document should specify which existing variants to reuse.

**Fix**: Add a section specifying the new `WebPlatformError` variants needed:

```rust
/// Alert rule not found (ALERT_001)
#[error("Alert rule not found: {0}")]
AlertRuleNotFound(String),  // -> 404, ALERT_001, showType 2

/// Notification channel config invalid (ALERT_002)
#[error("Channel config invalid: {0}")]
AlertChannelInvalid(String),  // -> 400, ALERT_002, showType 1

/// Test notification send failed (ALERT_003)
#[error("Notification send failed: {0}")]
AlertNotificationFailed(String),  // -> 502, ALERT_003, showType 1
```

---

## Minor Issues (Should Fix)

### M1. `AlertRule.cooldown_seconds` type should be `i64`, not `u64`

**Section**: 2.1, 2.2 — AlertRule struct

**Problem**: The struct uses `pub cooldown_seconds: u64`, but the database column is `INTEGER` (SQLite signed 64-bit). The existing codebase consistently uses `i64` for integer fields that come from the database (see `User`, `Project`, all repository traits). Using `u64` will require explicit casts when reading from SQLite.

**Fix**: Change `cooldown_seconds: u64` to `cooldown_seconds: i64` in `AlertRule` and `UpdateAlertRuleItem`.

---

### M2. `AlertHistoryRecord.context` should be `Option<HashMap<...>>`

**Section**: 1.1 — AlertHistoryRecord struct

**Problem**: The struct defines `pub context: HashMap<String, String>`, but the database column `context_json` is nullable (`TEXT` without `NOT NULL`). When `context_json` is NULL, deserialization to a non-optional HashMap will fail.

**Fix**: Change to `pub context: Option<HashMap<String, String>>` or ensure the repository layer always returns an empty HashMap for NULL values (document this explicitly).

---

### M3. Missing `ToSchema` derive on internal structs

**Section**: 6.2-6.5 — Internal architecture structs

**Problem**: Internal structs like `MetricSnapshot`, `RunningTask`, `ServiceHealthStatus`, `ConcurrencyMetrics`, `AlertEvent`, `NotificationResult` lack `ToSchema` derive. While these are internal and not directly exposed via API, some (like `AlertEvent`) are serialized and stored. More importantly, the `Severity` enum is used in both internal and API contexts but only has `Serialize, Deserialize` — it should also have `ToSchema` for OpenAPI generation.

**Fix**: Add `ToSchema` to `Severity` enum since it appears in API-facing structs.

---

### M4. `system_configs` table may not exist yet

**Section**: 5.1 — Migration SQL

**Problem**: The migration uses `INSERT OR IGNORE INTO system_configs (key, value, description)` but doesn't verify that the `system_configs` table exists. If Phase 5 migration runs before the table is created, it will fail.

**Fix**: Add a comment confirming that `system_configs` is created in V001 or V002, or add a defensive `CREATE TABLE IF NOT EXISTS` guard.

---

### M5. Cooldown persistence uses String timestamps, but CooldownManager uses `DateTime<Utc>`

**Section**: 6.4, 8.1

**Problem**: The `CooldownManager.restore_from_db` takes `Vec<(String, String, DateTime<Utc>)>`, but `AlertRepository.load_active_cooldowns` returns `Vec<(String, String, String)>` (three strings). The startup code in section 13.1 does the parse:

```rust
active_cooldowns.into_iter().map(|(rule_id, scope_key, expires_at)| {
    (rule_id, scope_key, expires_at.parse::<DateTime<Utc>>().unwrap())
}).collect()
```

Using `.unwrap()` on a parse from database data is unsafe — if the stored format is slightly different (e.g., missing timezone), this will panic at startup.

**Fix**: Use `.unwrap_or_else(|_| Utc::now())` or filter out unparseable entries with `.filter_map()` to prevent startup panics.

---

### M6. `evaluate_consecutive_failures` only checks projects with running tasks

**Section**: 6.5 — Rule evaluation logic

**Problem**: The `evaluate_consecutive_failures` method iterates `metrics.running_tasks.keys()`, which only includes projects that currently have running tasks. A project could have 3 consecutive failures and no currently running tasks (all failed and stopped). This project would never be evaluated.

**Fix**: The consecutive failure evaluation should iterate all projects that have recorded failures (from the MetricCollector's internal state), not just those with running tasks. Consider adding a method like `get_projects_with_failures()` to MetricCollector, or maintain a separate set of project IDs that have failure counts > 0.

---

### M7. No `task_failure` rule evaluation logic provided

**Section**: 6.5 — Rule evaluation

**Problem**: Section 6.5 provides evaluation logic for `task_timeout`, `service_crash`, `concurrency_saturation`, `consecutive_failures`, and `api_unreachable`, but omits `task_failure`. The `task_failure` rule ("Codex 任务异常退出且重试耗尽时触发") is event-driven rather than metric-snapshot-driven, so it doesn't fit the polling model.

**Fix**: Add a note explaining that `task_failure` is evaluated via event injection (when `record_task_failure` is called) rather than during the periodic evaluation cycle, or add the evaluation logic that checks for recently-failed tasks in the metric snapshot.

---

### M8. Channel PUT is "full replacement" — risky for concurrent admin edits

**Section**: 3.2 PUT /api/admin/alerts/channels

**Problem**: The document states channels are "全量替换" (full replacement). If two admins edit channels simultaneously, one's changes will be silently overwritten. Phase 4's concurrency config uses optimistic locking (`expected_previous`), but channels don't have this protection.

**Fix**: Consider adding an `updated_at` or version field for optimistic locking, or document that this is acceptable given the low frequency of channel configuration changes.

---

## Suggestions (Nice to Have)

### S1. Consider using an enum for `rule_id` instead of free-form String

The `rule_id` field is used as a String throughout, but the set of valid values is fixed (6 rules). A Rust enum with serde rename would provide compile-time safety and eliminate the need for runtime validation of unknown rule IDs.

### S2. Add a `GET /api/admin/alerts/:id` endpoint for single alert detail

The current design only has a list endpoint. For future use (e.g., linking from notifications back to the platform), a single-alert detail endpoint would be useful.

### S3. Consider batching DingTalk notifications

Section 11.3 mentions "钉钉 API 限流：同一机器人每分钟最多 20 条消息，引擎侧做聚合" but doesn't specify the aggregation mechanism. Consider documenting a batching window (e.g., collect alerts for 5 seconds, then send a single combined message).

### S4. Add health/status endpoint for the alert engine itself

A `GET /api/admin/alerts/engine-status` endpoint returning the engine's running state, last evaluation time, and error counts would help with operational monitoring.

### S5. The `encryption_key` for AES-256-GCM should be documented

Section 10.1 mentions reusing the existing `encryption_key` from AppState, but the document should note where this key is sourced (environment variable, config file) and how it's rotated.

---

## What's Good

- All 6 alert rules from the master design doc section 10.2 are fully covered with correct severity levels and thresholds.
- The DingTalk HMAC-SHA256 signing algorithm is correctly implemented (timestamp + newline + secret as the signing string, HMAC-SHA256 with secret as key, base64 encode, URL-encode in the query parameter).
- The cooldown/debounce mechanism is well-designed with per-rule scope keys, persistence for restart recovery, and in-memory DashMap for performance.
- The trait-based architecture cleanly separates concerns and matches the existing codebase patterns (async_trait, Send + Sync bounds).
- The shutdown/startup lifecycle is thoroughly documented.
- Security considerations are comprehensive (AES-256-GCM encryption, field masking, rate limiting, webhook URL validation).
