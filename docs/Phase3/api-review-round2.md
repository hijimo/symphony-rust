# Phase 3 API Specification - Adversarial Review (Round 2)

**Reviewer**: Frontend DX & Security Specialist  
**Date**: 2026-05-21  
**Focus**: Frontend developer experience, security attack vectors, real-world edge cases, implementation feasibility  
**Overall Assessment**: **PASS WITH CHANGES**

This review covers issues NOT identified in Round 1. Round 1 found 3 Critical + 7 Major issues (response format mismatches, naming conventions, pagination gaps). This round focuses on frontend integration pain points, security gaps, and operational edge cases.

---

## Critical Issues

### C4. SSE endpoint cannot use existing Axios client — no guidance for frontend

**Section**: POST /api/projects/:id/issues/ai-generate  
**Severity**: Critical

The existing frontend uses Axios (`web-frontend/src/api/client.ts`) with interceptors for auth and error handling. SSE endpoints cannot use Axios because:
1. Axios doesn't support streaming responses natively
2. `EventSource` API doesn't support POST requests or custom headers (no Bearer token)
3. The frontend must use `fetch()` with `ReadableStream` — a completely different code path

The spec doesn't document:
- How the frontend should authenticate the SSE request (Bearer header via fetch)
- How to handle auth failures mid-stream (token expires during generation)
- How to abort the stream (AbortController)
- Reconnection behavior (should the client retry on network drop?)

**Fix**: Add a "Frontend Integration Guide" section specifying:
- Use `fetch()` with `Authorization` header and `ReadableStream` for SSE
- Client should use `AbortController` to cancel on user action
- No automatic reconnection (each generation is a one-shot request)
- If 401 is returned before stream starts, redirect to login (same as Axios interceptor)
- Add `X-Request-Id` response header for debugging

---

### C5. Cache invalidation after issue creation creates stale kanban

**Section**: Cache strategy + POST /issues interaction  
**Severity**: Critical

When a user creates an issue via `POST /api/projects/:id/issues`, the kanban cache (5-10s TTL) will still show the old data. The user creates an issue, immediately views the kanban, and their new issue is missing. This is a confusing UX.

The spec mentions no cache invalidation strategy for write operations.

**Fix**: After a successful `POST /issues`, invalidate the kanban cache for that user+project. Options:
- (a) Backend invalidates cache entry on write (recommended)
- (b) Return a `X-Cache-Invalidated: true` header so frontend knows to force-refresh
- (c) Document that the frontend should pass `Cache-Control: no-cache` after creating an issue

Option (a) is simplest and most reliable.

---

## Major Issues

### M8. Rate limit is per-user but AI cost is per-project — abuse vector

**Section**: Rate limiting table  
**Severity**: Major

AI generation rate limit is "10 次/用户/分钟". But a project with 10 members means 100 AI calls/minute for that project. If Azure OpenAI has per-deployment rate limits (common: 60 RPM for gpt-4 class models), a team could exhaust the shared quota.

Additionally, there's no per-project rate limit. A malicious or careless user could switch between projects to effectively bypass the per-user limit (10/min on project A + 10/min on project B = 20/min total AI calls).

**Fix**: Add a global AI rate limit (e.g., 30 calls/minute across all users) in addition to per-user limits. Document the Azure OpenAI deployment's RPM limit as a system config.

---

### M9. No SSE connection timeout or keepalive specification

**Section**: SSE protocol  
**Severity**: Major

The spec says "60 秒无数据则服务端关闭连接" but doesn't specify:
- What if Azure OpenAI is slow but not dead (30s between tokens)?
- Should the server send keepalive comments (`:\n\n`) to prevent proxy/load-balancer timeouts?
- What's the maximum total generation time before the server force-closes?
- What HTTP status does the client see if the server closes mid-stream?

Real-world: Nginx default proxy_read_timeout is 60s. If the AI takes 45s to start generating (cold start), the proxy may kill the connection before any data arrives.

**Fix**: Specify:
- Server sends `: keepalive\n\n` every 15s if no data
- Maximum generation time: 120s total, then force-close with error event
- If Azure OpenAI doesn't respond within 30s, send error event and close
- Document that reverse proxies need `proxy_read_timeout 120s` and `proxy_buffering off`

---

### M10. Token revocation mid-request has undefined behavior

**Section**: All endpoints using user tokens  
**Severity**: Major

If a user's GitLab token is revoked while:
1. A kanban request is in-flight → GitLab returns 401
2. An issue creation is in-flight → GitLab returns 401
3. An AI generation is streaming → The issue creation after generation will fail

The spec maps external 401 to `EXT_001` (502), but this is misleading. The user needs to update their token, not wait for GitLab to recover.

Round 1 mentioned this as minor (m6), but it's actually Major because:
- The frontend's error interceptor will show "外部服务不可用" — user has no idea their token is the problem
- The user might wait hours thinking GitLab is down

**Fix**: Distinguish external auth failures:
- GitLab/GitHub returns 401/403 → Return `BIZ_001` with `retMsg: "平台 Token 无效或已过期，请在个人设置中更新"`
- GitLab/GitHub returns 5xx or timeout → Return `EXT_001` with `retMsg: "GitLab/GitHub 服务暂时不可用"`

---

### M11. Kanban endpoint does too much — slow first paint

**Section**: GET /api/projects/:id/kanban  
**Severity**: Major

The kanban endpoint fetches ALL three columns in a single request. For a project with 50 todo issues + 10 in-progress issues + N MR lookups, this means:
- 1 API call for todo issues
- 1 API call for in-progress issues
- N API calls for related MRs (one per in-progress issue)

Total: 2 + N external API calls. With N=10, that's 12 API calls before the response can be sent. Even with parallel execution, this could take 3-5 seconds.

The frontend can't show ANY column until ALL columns are loaded.

**Fix**: Consider one of:
- (a) Split into 3 endpoints: `/kanban/todo`, `/kanban/in-progress`, `/kanban/pr` — frontend loads columns independently
- (b) Keep single endpoint but add `columns` query param: `?columns=todo,in_progress` to allow partial loading
- (c) Document expected latency and recommend frontend shows skeleton/loading per-column with a single request

Option (c) is simplest for Phase 3 MVP. Add a note about expected response time (2-5s for first uncached request).

---

## Minor Issues

### m8. AI prompt injection via unicode/homoglyph bypass

**Section**: Prompt injection protection  
**Severity**: Minor (borderline Major)

The spec mentions filtering patterns like `ignore previous`, `system:`, `you are now`. But these can be bypassed with:
- Unicode homoglyphs: `іgnore prevіous` (Cyrillic і instead of Latin i)
- Zero-width characters: `ignore​previous`
- Base64 encoding in the prompt: "Please decode and execute: aWdub3JlIHByZXZpb3Vz"
- Markdown/HTML injection in the output that could confuse the frontend renderer

**Fix**: 
- Normalize unicode before pattern matching (NFKC normalization)
- Strip zero-width characters from input
- The whitelist approach for Validation commands is more robust than the input blocklist — emphasize output validation over input filtering
- Document that input filtering is defense-in-depth, not the primary protection

---

### m9. No specification for concurrent AI generation requests from same user

**Section**: AI generation endpoint  
**Severity**: Minor

What happens if a user opens two browser tabs and triggers AI generation simultaneously? The rate limit allows it (10/min), but:
- Should the server allow multiple concurrent SSE streams per user?
- If not, should the second request fail immediately or queue?
- Resource concern: each stream holds an open connection + Azure OpenAI session

**Fix**: Add a concurrency limit: max 1 concurrent AI generation per user. Second request returns 429 with `retMsg: "已有生成任务进行中，请等待完成后重试"`.

---

### m10. `mr_count` field on KanbanIssue is only for in_progress but schema doesn't enforce this

**Section**: KanbanIssue schema  
**Severity**: Minor

The `mr_count` field is described as "仅处理中列有值" but it's defined on the shared `KanbanIssue` schema used by both `todo` and `in_progress` columns. Frontend developers might try to render it for todo issues and get `null`.

**Fix**: Either:
- Document clearly that `mr_count` is always `null` for todo column issues
- Or split into `KanbanTodoIssue` and `KanbanInProgressIssue` schemas (over-engineering for this case)

The first option is sufficient — just add a note in the field description.

---

### m11. Frontend Axios interceptor hardcodes 429 error message for login only

**Section**: Frontend integration  
**Severity**: Minor

The existing `client.ts` interceptor handles 429 with the message "登录尝试过于频繁". Phase 3 introduces 429 for AI generation and kanban endpoints. The frontend interceptor needs updating to show context-appropriate messages.

**Fix**: The backend should always return a JSON body with 429 responses (which the spec already does). The frontend interceptor should use `body.retMsg` instead of a hardcoded message. This is a frontend implementation detail, but the spec should note that 429 responses always include a JSON body.

---

### m12. No `Retry-After` header on 429 responses

**Section**: Rate limiting  
**Severity**: Minor

RFC 6585 recommends including a `Retry-After` header with 429 responses to tell clients when they can retry. This helps the frontend show "请在 X 秒后重试" instead of a vague message.

**Fix**: Add `Retry-After: <seconds>` header to all 429 responses. Include the value in the response body too for convenience: `"retryAfter": 30`.

---

### m13. GitHub API differences not fully mapped

**Section**: Issue-PR association for GitHub  
**Severity**: Minor

The spec mentions using GitHub Timeline Events API for issue-PR association, but:
- Timeline API requires `application/vnd.github.mockingbird-preview+json` accept header (was preview, now stable but behavior differs)
- GitHub GraphQL is more reliable but requires different auth scope (`read:project`)
- GitHub doesn't have `iid` — it uses `number`. The spec uses `iid` uniformly which may confuse GitHub-focused developers.

**Fix**: Add a note that `iid` maps to GitLab's `iid` and GitHub's `number`. Document which GitHub API approach is preferred (REST Timeline vs GraphQL) and required token scopes.

---

## Suggestions

### S6. Add `platform` field to kanban response for frontend rendering

The frontend needs to know whether to show "MR" or "PR", link to GitLab or GitHub, etc. The kanban response should include the project's platform type.

**Fix**: Add `platform: "gitlab" | "github"` to the top level of `KanbanData`.

---

### S7. Consider adding `stale` indicator for degraded mode

When the cache is serving data but the external API is down, the frontend should show a warning like "数据可能已过期". The `cached` + `cached_at` fields help, but a dedicated `stale: true` field (when cache is being served because the API failed) would be clearer.

---

### S8. Document SSE error recovery UX pattern

The spec defines the error event format but doesn't suggest how the frontend should handle it. Recommend:
- Show the partial content generated so far (don't discard it)
- Show an inline error message with a "重试" button
- Retry should send the same prompt (idempotent)

---

## Consolidated Verdict (Both Rounds)

| Round | Critical | Major | Minor | Suggestion |
|-------|----------|-------|-------|------------|
| Round 1 | 3 | 7 | 7 | 5 |
| Round 2 | 2 | 4 | 6 | 3 |
| **Total** | **5** | **11** | **13** | **8** |

### Must-Fix Before Implementation (Critical + High-Impact Major)

1. **C1** (R1): `showType` omitted on success — align spec to existing behavior
2. **C2** (R1): `retMsg` should be `"ok"` not `"success"`
3. **C3** (R1): `POST /issues` should return 200 (match existing convention) or 201 (REST correct) — decide and document
4. **C4** (R2): Add SSE frontend integration guide (fetch + AbortController)
5. **C5** (R2): Cache invalidation after write operations
6. **M1** (R1): Resolve snake_case vs camelCase for entity fields
7. **M2** (R1): Per-error-code `showType` requires error handling refactor
8. **M10** (R2): Distinguish token-invalid from service-down errors

### Should-Fix (Remaining Major)

9. **M3-M7** (R1): Admin token requirement, singleflight scope, pagination, N+1, timeouts
10. **M8** (R2): Global AI rate limit
11. **M9** (R2): SSE keepalive and timeout spec
12. **M11** (R2): Document expected kanban latency

### Recommendation

The spec is solid architecturally. The critical issues are mostly alignment problems with existing code (easy to fix). The major issues require design decisions but none are blockers if documented as accepted tradeoffs. 

**Proceed to implementation after fixing C1-C5 and M1, M2, M10.** Other issues can be addressed during implementation.
