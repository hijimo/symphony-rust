# Phase 4 API Design - Adversarial Review

**Reviewer**: Rust Agent 2 (Adversarial API Reviewer)  
**Document**: `docs/Phase4/api-design.md`  
**Date**: 2026-05-21  
**Verdict**: NEEDS_REVISION

---

## Summary

The Phase 4 API design is well-structured and covers the core requirements (Token isolation, concurrency control, author attribution). However, the review identified **4 critical**, **7 major**, and **5 minor** issues that must be addressed before implementation.

Key concerns:
1. Security: SSE JWT in query parameter is logged by proxies/CDNs; timing attack on token validation
2. Race conditions: Concurrent config updates lack optimistic locking; stale snapshot reads during watcher restart
3. Consistency: New error code `TOKEN_002` conflicts with existing `TOKEN_001` semantics; `EXT_002` already used for rate limiting
4. Completeness: Missing admin stats endpoint (doesn't exist yet), missing pagination on concurrency events history

---

## Critical Issues

### C1: SSE JWT Token Exposure via Query Parameter (Section 2.6)

**Problem**: The SSE endpoint passes JWT via `?token=<jwt>` query parameter. Despite the note about "not recording to logs," this is insufficient:
- Reverse proxies (nginx, Cloudflare) log full URLs by default
- Browser history stores the URL
- HTTP Referer headers may leak the token to third-party resources
- Server access logs in production environments typically capture query strings

**Impact**: Token leakage leading to session hijacking.

**Fix**: Use a short-lived, single-use ticket pattern:
1. Client calls `POST /api/admin/concurrency/events/ticket` to get a one-time ticket (valid 30s)
2. Client connects to SSE with `?ticket=<one-time-ticket>`
3. Server validates and immediately invalidates the ticket

Alternatively, use the first SSE message as an auth handshake (client sends JWT in the first `EventSource` message via a custom protocol).

---

### C2: Token Validation Timing Attack (Section 1.2)

**Problem**: `POST /api/user/config/validate-token` accepts a raw token in the request body and calls the platform API. If the platform API response time differs between valid/invalid tokens, an attacker who intercepts the request can infer token validity through timing analysis.

More critically: the endpoint accepts ANY token (not just the user's own). A malicious authenticated user could validate stolen tokens from other users by submitting them to this endpoint.

**Impact**: Information disclosure; potential token enumeration.

**Fix**: 
- Restrict the endpoint to only validate the token currently stored in the user's own config (don't accept arbitrary tokens in the body)
- Or: if the use case requires validating before saving, add constant-time response padding and rate limit to 3 req/min

---

### C3: Migration File Numbering Collision (Section 5.1)

**Problem**: The migration is named `V003__phase4_concurrency.sql`, but the existing migrations are `V001__init_schema.sql` and `V002__projects_extend.sql`. Phase 3 likely needs its own migration (for AI generation audit logs, etc.). If Phase 3 already uses V003, this will collide.

**Impact**: Migration failure at startup; potential data corruption if applied out of order.

**Fix**: Verify the actual next available migration number. If Phase 3 has not added migrations, V003 is correct. If it has, use the next available number. Document the dependency explicitly.

---

### C4: `projects.max_concurrent_agents` Column Already Exists But Not Documented (Section 5.1)

**Problem**: The migration in Section 5.1 does NOT add the `max_concurrent_agents` column to the `projects` table, yet Section 2.5 references `projects.max_concurrent_agents`. The existing codebase already has this column (confirmed in `sqlite.rs:360`), but the API design document doesn't acknowledge this. The `PUT /api/projects/:id/concurrency` endpoint says "Update `projects.max_concurrent_agents` column" without a migration to add it.

**Impact**: Confusion during implementation; potential assumption that the column needs to be created.

**Fix**: Add a note in Section 5.1 clarifying that `projects.max_concurrent_agents` already exists from V002 migration. No new ALTER TABLE needed for this column.

---

## Major Issues

### M1: Error Code `TOKEN_002` Semantics Overlap (Section 1.4)

**Problem**: The document introduces `TOKEN_002` for "Owner Token not configured." However, the existing `TOKEN_001` already covers "Platform token is invalid or expired" (see `error.rs:35`). The distinction between "not configured" vs "invalid" is useful, but the error code namespace needs clarification:
- `TOKEN_001` = token invalid/expired (existing)
- `TOKEN_002` = token not configured (new)

But in Section 3.3, the contributors endpoint references `TOKEN_001` for "user Token invalid" — this is correct but creates confusion because the same error code is used for both "user's own token" and "owner's token" scenarios.

**Fix**: Keep `TOKEN_002` for owner-specific token issues. Add a clear error code table in the document showing the full TOKEN_xxx namespace.

---

### M2: No Optimistic Locking on Concurrent Config Updates (Section 2.3, 2.5)

**Problem**: `PUT /api/admin/concurrency/config` and `PUT /api/projects/:id/concurrency` have no concurrency control. If two admins simultaneously update `global_max`:
1. Admin A reads current value = 5
2. Admin B reads current value = 5
3. Admin A sets to 8
4. Admin B sets to 3 (unaware of A's change)

The `previous_value` in the response is informational only and doesn't prevent lost updates.

**Impact**: Silent configuration overwrites; potential operational confusion.

**Fix**: Add an `expected_previous` optional field to the request. If provided and doesn't match current value, return `BIZ_003` (conflict). This implements optimistic locking without adding complexity for simple cases.

---

### M3: SSE Connection Limit Bypass via Reconnection (Section 2.6)

**Problem**: The document specifies "max 10 concurrent SSE connections" but doesn't address:
- What happens when a client disconnects and immediately reconnects (connection slot freed?)
- Can a single user open 10 connections?
- No per-user connection limit specified

**Impact**: A single admin could exhaust all 10 SSE slots, denying access to other admins.

**Fix**: Add per-user SSE connection limit (e.g., 2 per user). Track connections by user_id, not just total count.

---

### M4: Stale Data During Watcher Restart (Section 9.2)

**Problem**: If the Web Platform restarts, the `ConcurrencyManager` is empty. The document mentions "load from `concurrency_snapshots` table" but:
1. The snapshot in DB may be stale (last written 5s+ ago)
2. Between startup and first watcher poll (up to 5s), the API returns stale/zero data
3. The `GET /api/admin/concurrency` endpoint has no indication that data may be stale

**Impact**: Incorrect concurrency decisions during startup window; admin sees misleading zero-agent state.

**Fix**: Add a `data_freshness` field to the concurrency response indicating seconds since last successful poll. If > 10s, frontend should show a "data may be stale" warning.

---

### M5: Contributors Endpoint Aggregation is Incomplete (Section 3.3)

**Problem**: The implementation note says "fetch last 100 Issues and 100 MRs" — this is an arbitrary limit that will miss contributors who only contributed earlier. For active projects with many issues, this gives an inaccurate picture.

Additionally, the `issue_count` and `mr_count` fields are misleading because they only reflect the last 100 items, not the true total.

**Impact**: Inaccurate contributor statistics; users may not appear in the list.

**Fix**: 
- Rename fields to `recent_issue_count` and `recent_mr_count` to clarify scope
- Or: document the limitation clearly in the API response (add a `scope` field: `"last_100_items"`)
- Consider using platform-native contributor APIs where available (GitLab has `/projects/:id/repository/contributors`)

---

### M6: `query_hash` Function Doesn't Include New `author` Parameter (Section 3.2)

**Problem**: The existing `query_hash` function in `kanban.rs` hashes `todo_limit`, `assignee`, `labels`, `search` — but NOT the new `author` field. If `author` is added to `KanbanQuery` without updating `query_hash`, two requests with different `author` values will share the same cache entry.

**Impact**: Users see incorrect filtered data from cache; data isolation violation.

**Fix**: Update `query_hash` to include `query.author.hash(&mut hasher)`.

---

### M7: `list_issues_with_author` Trait Method is Redundant (Section 10.1)

**Problem**: The document proposes adding `list_issues_with_author` as a separate method on `GitPlatformClient`. However, the existing `ListIssuesOptions` struct can simply be extended with an `author` field (which the document also proposes in Section 10.2). Having both a new method AND an extended struct is contradictory.

The existing `list_issues` method with the extended `ListIssuesOptions` is sufficient.

**Impact**: Confusing API surface; implementors must maintain two methods that do the same thing.

**Fix**: Remove `list_issues_with_author` from the trait extension. Only extend `ListIssuesOptions` with the `author` field and use the existing `list_issues` method.

---

## Minor Issues

### m1: Inconsistent Field Naming in SSE Events (Section 2.6)

**Problem**: The `ConcurrencyEvent` enum uses `#[serde(tag = "type", rename_all = "snake_case")]` but the SSE event names in the protocol section use `snake_case` too (e.g., `event: agent_started`). However, the JSON data fields use `camelCase` (from `#[serde(rename_all = "camelCase")]` on the inner structs). This means the `type` field in JSON will be `snake_case` while other fields are `camelCase` — inconsistent within the same JSON object.

**Fix**: Either use `camelCase` for the tag value too (e.g., `"type": "agentStarted"`) or document this intentional inconsistency.

---

### m2: Missing `EXT_002` Error Code Documentation (Section 1.2)

**Problem**: The token validation endpoint lists `EXT_002 (429): Rate limit` as an error response, but `EXT_002` in the existing codebase maps to `RateLimited` with HTTP 429. The new error code table in the document only lists `CONCURRENCY_001`, `CONCURRENCY_002`, and `TOKEN_002`. The reuse of `EXT_002` is correct but should be explicitly noted as "existing code, reused here."

**Fix**: Add a note clarifying that `EXT_002` is an existing error code being reused.

---

### m3: `concurrency_snapshots` Table Has Redundant Index (Section 5.1)

**Problem**: The table has `UNIQUE(project_id)` constraint AND a separate `CREATE INDEX idx_concurrency_snapshots_project ON concurrency_snapshots(project_id)`. The UNIQUE constraint already creates an implicit index in SQLite.

**Fix**: Remove the explicit index creation for `project_id` on `concurrency_snapshots`.

---

### m4: Bot Detection Heuristic is Fragile (Section 3.3)

**Problem**: Bot identification relies on username containing `bot`, `symphony`, or `codex`. This will false-positive on usernames like "robotics_engineer" or "symbot_admin". It will also miss bots with non-standard names.

**Fix**: Make bot detection configurable per-project (store bot usernames in project config). Use the heuristic only as a fallback.

---

### m5: No Pagination on Concurrency Events History

**Problem**: The `concurrency_events` table will grow continuously (every agent start/stop is recorded). The `ConcurrencyHistory` struct in Section 2.4 only shows today's stats, but there's no endpoint to query historical events with pagination. The admin may need to investigate past throttling events.

**Fix**: Consider adding `GET /api/admin/concurrency/history?pageNo=1&pageSize=20&project_id=1&date_from=...` or document this as a Phase 5 feature.

---

## Positive Observations

1. The `ConcurrencyManager` design with `DashMap` + `AtomicI64` is well-suited for the read-heavy workload
2. The file-based status protocol (`/tmp/symphony-{project_id}-status.json`) with atomic rename is a pragmatic choice for IPC
3. The `logical_author` attribution for Codex-created PRs is a thoughtful UX feature
4. Race condition handling table in Section 9.3 shows good awareness of concurrency issues
5. The broadcast channel with 256 capacity and lag-skip semantics is appropriate for SSE

---

## Verdict: NEEDS_REVISION

The critical issues (C1, C2) are security concerns that must be resolved before implementation. C3 and C4 are correctness issues that will cause build/migration failures. The major issues (M1-M7) affect maintainability and correctness but are not blockers if documented as known limitations.

Recommended action: Fix all Critical and Major issues, then proceed to implementation.
