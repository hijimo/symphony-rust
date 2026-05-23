# Phase 5 API Design Review — Round 2 (Final)

## Verdict: APPROVED FOR IMPLEMENTATION (with minor fixes required)

The Round 1 critical issues have been substantially addressed. The document is now consistent with existing codebase patterns for the most part. A few residual type inconsistencies remain that should be fixed before implementation begins, but none are architectural blockers.

---

## 1. Verification of Round 1 Fixes

### C1. Pagination type mismatch — CONFIRMED FIXED

The document now correctly states (line 203-206):
> NOTE: Use existing PaginationData<AlertHistoryRecord> from models/response.rs instead of a custom struct.

`AlertHistoryQuery` uses `Option<i64>` for `page_no` and `page_size`. This matches the existing `PaginationData<T>` which uses `i64` for all fields.

### C2. Query parameter naming — CONFIRMED FIXED

`AlertHistoryQuery` no longer has `#[serde(rename_all = "camelCase")]`. Field names are snake_case (`page_no`, `page_size`, `rule_id`, `project_id`, `start_time`, `end_time`). This matches the existing `KanbanQuery` and `ProjectListQuery` patterns.

**However**, there is a new inconsistency introduced: the query parameter table (lines 80-88) documents the parameters as camelCase (`pageNo`, `pageSize`, `ruleId`, `projectId`, `startTime`, `endTime`), but the Rust struct uses snake_case without rename. The actual accepted query parameters will be `page_no`, `page_size`, `rule_id`, etc. See NEW issue N1 below.

### C3. JSON response field naming — CONFIRMED FIXED

All JSON response examples now use camelCase (`ruleId`, `projectId`, `firedAt`, `notificationStatus`, etc.), matching the `#[serde(rename_all = "camelCase")]` on response structs.

### C4. MetricCollector missing `record_service_crash` — CONFIRMED FIXED

The method is now present in the `MetricCollector` trait (line 1089):
```rust
async fn record_service_crash(&self, project_id: i64, exit_code: i32);
```

### C5. `issue_iid` type — CONFIRMED FIXED

`RunningTask.issue_iid` is now `i64` (line 1043), and `record_task_failure` takes `issue_iid: i64` (line 1071). This matches the existing `ConcurrencyRepository` trait which uses `Option<i64>`.

**Note**: The existing `ConcurrencyEvent` enum in `models/concurrency.rs` uses `Option<u64>` for `issue_iid` (lines 68, 76 of that file). This is an existing inconsistency in the codebase (the repository trait uses `Option<i64>` but the SSE event model uses `Option<u64>`). Phase 5's choice of `i64` is correct for database interop.

### C6. Error codes not mapped to WebPlatformError — CONFIRMED FIXED

Section "新增 WebPlatformError 变体" (lines 38-52) now explicitly defines:
- `AlertRuleNotFound(String)` -> 404, ALERT_001, showType 2
- `AlertChannelInvalid(String)` -> 400, ALERT_002, showType 1
- `AlertNotificationFailed(String)` -> 502, ALERT_003, showType 1

These follow the exact pattern of existing variants in `error.rs`.

---

## 2. NEW Issues Found

### N1 (Minor): Query parameter table documents camelCase but struct accepts snake_case

**Section**: 1.1 GET /api/admin/alerts — Query Parameters table

**Problem**: The table at lines 80-88 lists parameters as `pageNo`, `pageSize`, `ruleId`, `projectId`, `startTime`, `endTime`. But the `AlertHistoryQuery` struct (line 172) uses snake_case fields without `rename_all`, meaning the actual accepted query parameters are `page_no`, `page_size`, `rule_id`, `project_id`, `start_time`, `end_time`.

**Impact**: Frontend developers will use the wrong parameter names if they follow the table.

**Fix**: Update the query parameter table to use snake_case names, or add `#[serde(alias = "pageNo")]` attributes to support both. Given that existing endpoints (kanban, projects) use snake_case query params, the table should be corrected to snake_case.

---

### N2 (Minor): `cooldown_seconds` type mismatch between trait and struct

**Section**: 6.2 RuleEvaluator trait (line 1138) and 6.4 CooldownManager (line 1559)

**Problem**: 
- `AlertRule.cooldown_seconds` is `i64` (line 339) — correct for DB interop
- `RuleEvaluator::mark_fired` takes `cooldown_seconds: u64` (line 1138)
- `CooldownManager::mark_fired` takes `cooldown_seconds: u64` (line 1559)
- The evaluation code passes `rule.cooldown_seconds` (i64) directly to `mark_fired` (u64) — this won't compile without a cast

Similarly, `AlertRepository::update_alert_rule` takes `cooldown_seconds: Option<u64>` (line 1934) but the struct field is `i64`.

**Fix**: Change `mark_fired` and `update_alert_rule` to accept `i64` for consistency, or document the `as u64` cast. Since the value is validated to be in [60, 3600], the cast is safe, but using `i64` throughout is cleaner.

---

### N3 (Minor): `AlertHistoryStore::query_history` references undefined type

**Section**: 6.2 AlertHistoryStore trait (line 1322)

**Problem**: The method signature is:
```rust
async fn query_history(&self, query: &AlertHistoryQuery) -> Result<AlertHistoryListResponse>;
```

But `AlertHistoryListResponse` was removed per C1 fix. The document says to use `PaginationData<AlertHistoryRecord>` instead. The `AlertRepository` trait (line 1974) correctly returns `Result<(Vec<AlertHistoryRecord>, u64)>`, but the `AlertHistoryStore` trait still references the old type.

**Fix**: Change to:
```rust
async fn query_history(&self, query: &AlertHistoryQuery) -> Result<(Vec<AlertHistoryRecord>, i64)>;
```
(Using `i64` for total_count to match `PaginationData`.)

---

### N4 (Minor): `api_health` uses `u64` but should use `i64` for consistency

**Section**: 6.2 MetricSnapshot (line 1035)

**Problem**: `pub api_health: HashMap<String, u64>` — this is an internal struct not stored in DB, so `u64` is technically fine. However, `get_consecutive_failures` and `get_api_consecutive_failures` also return `u64` (lines 1083, 1086). The threshold comparison uses `v.as_u64()` from serde_json which returns `Option<u64>`, so this is internally consistent.

**Verdict**: Acceptable. Internal metrics don't need to match DB types. No fix needed.

---

### N5 (Minor): `unwrap()` on DateTime parse during startup — still present

**Section**: 13.1 Startup flow (line 2145)

**Problem**: Round 1 flagged this as M5 but it was not fixed:
```rust
expires_at.parse::<DateTime<Utc>>().unwrap()
```

If the database contains a malformed timestamp, this panics at startup.

**Fix**: Use `filter_map` to skip unparseable entries:
```rust
cooldown_manager.restore_from_db(
    active_cooldowns.into_iter().filter_map(|(rule_id, scope_key, expires_at)| {
        expires_at.parse::<DateTime<Utc>>().ok().map(|dt| (rule_id, scope_key, dt))
    }).collect()
);
```

---

### N6 (Minor): `consecutive_failures` evaluation only checks projects with running tasks — still present

**Section**: 6.5 evaluate_consecutive_failures (line 1773)

**Problem**: Round 1 flagged this as M6 but it was not fixed. The code iterates `metrics.running_tasks.keys()`, which misses projects that have accumulated failures but no currently running tasks.

**Fix**: Add a method to MetricCollector like `get_projects_with_failures() -> Vec<i64>` and iterate that instead, or maintain the failure counts in the MetricSnapshot directly.

---

### N7 (Minor): `task_failure` evaluation mechanism not documented — still present

**Section**: 6.5

**Problem**: Round 1 flagged this as M7. The `task_failure` rule is event-driven (fires when `record_task_failure` is called and retries are exhausted), not metric-snapshot-driven. But there's no code showing how `record_task_failure` triggers an alert. The current design only evaluates rules during the periodic `evaluate_cycle`.

**Fix**: Add a note or code showing that `record_task_failure` should directly inject an `AlertEvent` (bypassing the periodic evaluation), or add a `recently_failed_tasks` field to `MetricSnapshot` that the evaluator checks.

---

### N8 (Suggestion): `alert_history` table already exists in V001

**Section**: 5.1 Migration V004

**Problem**: Looking at `V001__init_schema.sql` (lines 86-102), the `alert_history` table already exists with the same schema (minus `created_at` column and some indexes). The Phase 5 migration's `CREATE TABLE alert_history` will fail because the table already exists.

**Fix**: The migration should use `CREATE TABLE IF NOT EXISTS` or, better, only add the missing columns/indexes. Specifically:
- Add `created_at TEXT NOT NULL DEFAULT (datetime('now'))` column (ALTER TABLE)
- Add the missing indexes (`idx_alert_history_rule`, `idx_alert_history_status`, composite indexes)
- The `alert_rules`, `notification_channels`, and `alert_cooldowns` tables are new and can use `CREATE TABLE`

This is a **critical implementation detail** — the migration will fail as written.

---

### N9 (Suggestion): `watcher.rs` integration point needs access to AlertManager

**Section**: 15.1 Integration with ProcessManager

**Problem**: The current `spawn_watcher` function (in `process_manager/watcher.rs`) takes `ProcessManager`, `SqliteRepository`, `encryption_key`, `symphony_bin`, and `workspace_root` as parameters. It does NOT have access to `AlertManager` or `MetricCollector`. The proposed integration code:
```rust
alert_manager.metric_collector.record_service_crash(project_id, exit_code).await;
```
requires passing `AlertManager` (or at minimum `Arc<dyn MetricCollector>`) into the watcher.

**Fix**: Document that `spawn_watcher` signature must be extended to accept `Arc<dyn MetricCollector>`, or that the watcher should be refactored to receive it through a shared state mechanism.

---

### N10 (Suggestion): Channel PUT "全量替换" race condition — still undocumented

**Section**: 3.2

**Problem**: Round 1 flagged this as M8. The document still doesn't address the race condition for concurrent admin edits. While the frequency is low, it should at minimum be documented as a known limitation.

**Fix**: Add a brief note: "Note: Channel configuration uses full replacement without optimistic locking. Concurrent edits by multiple admins may result in lost updates. This is acceptable given the low frequency of channel configuration changes."

---

## 3. Edge Case Analysis

### DingTalk webhook not configured but alerts fire

The design handles this correctly. The `ChannelRouter::route()` method selects channels based on severity filter. If no channels are configured or enabled, `route()` returns an empty Vec, and `dispatch()` simply returns no results. The alert is still recorded in history with `notification_status` = NULL (or could be set to "suppressed"). This is acceptable.

### Empty alert_rules table at startup

The migration pre-populates 6 rules via INSERT statements. If somehow the table is empty, `get_all_alert_rules()` returns an empty Vec, the `DefaultRuleEvaluator` has no rules to evaluate, and the engine runs harmlessly doing nothing. This is safe.

### Race condition: two alerts fire simultaneously for same rule

The `CooldownManager` uses `DashMap` which provides atomic per-key operations. `is_cooling_down` + `mark_fired` is NOT atomic as a pair — two threads could both pass `is_cooling_down` before either calls `mark_fired`. However, since the evaluation loop is single-threaded (one `evaluate_cycle` at a time via `tokio::select!` on interval), this race cannot occur during normal periodic evaluation. It could theoretically occur if event-driven alerts (task_failure) fire concurrently with periodic evaluation, but the worst case is a duplicate notification, which is acceptable.

### Metric collection tick fires while previous is still running

The `AlertEngine::run()` uses `tokio::time::interval` with `tokio::select!`. If `evaluate_cycle` takes longer than the interval, the next tick fires immediately after the previous completes (interval's default `MissedTickBehavior::Burst`). This could cause rapid successive evaluations. Consider documenting that `MissedTickBehavior::Delay` should be used to prevent this.

---

## 4. Summary

| Category | Count | Status |
|----------|-------|--------|
| Round 1 Critical fixes verified | 6/6 | All confirmed |
| New critical issues | 1 (N8 - migration conflict) | Must fix before implementation |
| New minor issues | 6 (N1-N3, N5-N7) | Should fix |
| New suggestions | 3 (N9, N10, interval behavior) | Nice to have |

### Final Verdict: APPROVED FOR IMPLEMENTATION

The design is architecturally sound and well-aligned with the existing codebase. The one critical issue (N8 — migration conflict with existing `alert_history` table) must be resolved in the migration script before implementation, but does not require changes to the API design itself. All other issues are minor type mismatches or documentation gaps that can be addressed during implementation.
