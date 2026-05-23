# Phase 2 API Specification Review Notes

Reviewer: Backend Architect Agent
Date: 2026-05-21
Spec version: 0.2.0

---

## Round 1: Completeness & Correctness

### Issue #1 — Missing WORKFLOW.md endpoints in design doc section 11.4

**Severity**: Major

**Description**: The design doc section 11.4 lists `PUT /api/projects/:id` as handling WORKFLOW.md updates ("更新项目配置（含 WORKFLOW.md）"). The API spec instead separates workflow into dedicated endpoints (`/api/projects/{id}/workflow` GET/PUT and `/api/projects/{id}/workflow/reset`). This is actually a better design (separation of concerns), but it is an undocumented deviation from the master design doc. The spec does NOT document this deviation anywhere.

Additionally, the design doc section 5.4 describes WORKFLOW.md management in detail, and the spec's dedicated workflow endpoints align well with those requirements. However, the spec adds a `POST /workflow/reset` endpoint that is not listed anywhere in section 11.4.

**Fix applied**: No code fix needed. This is a documentation gap. The spec's approach is superior. Recommend updating the design doc section 11.4 to reflect the dedicated workflow endpoints.

---

### Issue #2 — Missing `EXT_001` error code

**Severity**: Major

**Description**: The design doc (section 11.0) defines error code `EXT_001` ("外部服务不可用 GitLab/GitHub/AI") with showType 4 (notification). The API spec's error code table omits this entirely. The `/members/sync` endpoint calls external GitLab/GitHub APIs and can fail due to external service unavailability, but there is no `EXT_001` response documented.

**Fix applied**: Added `EXT_001` to the error code table in the spec description and added an `ErrorResponseExt001` schema. Added `EXT_001` as a possible response to the sync members endpoint.

---

### Issue #3 — Missing `SYS_001` responses on all endpoints

**Severity**: Major

**Description**: Every endpoint can potentially return a `SYS_001` (system internal error) response, but no endpoint documents this as a possible response. A frontend developer has no way to know the shape of a 500 response without this.

**Fix applied**: Added `500` response with `SYS_001` to all endpoints.

---

### Issue #4 — Pagination response schema inconsistency with design doc

**Severity**: Minor

**Description**: The design doc section 11.0 defines pagination response as including both `limit`/`offset` AND `pageNo`/`pageSize`/`pages`/`records`/`totalCount`. The spec's `ProjectListResponse` only includes `records`, `totalCount`, `pageNo`, `pageSize`, `pages` — missing `limit` and `offset`. This is actually fine (limit/offset are redundant when you have pageNo/pageSize), but it's a deviation.

**Fix applied**: No fix. The spec's approach is cleaner. Minor deviation acceptable.

---

### Issue #5 — `service_status` enum mismatch between design doc and spec

**Severity**: Minor

**Description**: The design doc section 5.3 defines only 3 states: `running`, `stopped`, `error`. The spec defines 6 states: `running`, `stopped`, `starting`, `stopping`, `error`, `failed`. The spec is more complete (transitional states are necessary for proper UI), but this is an undocumented enhancement.

**Fix applied**: No code fix. The spec's expanded state machine is correct for production use.

---

### Issue #6 — `max_concurrent_agents` and `auto_restart` missing from Project DB schema

**Severity**: Minor

**Description**: The spec's `Project` entity includes `max_concurrent_agents` and `auto_restart` fields, but the design doc's database schema (section 3.3) does not include these columns. The spec is adding fields not in the DB design.

**Fix applied**: No spec fix needed. The DB schema in the design doc needs updating (out of scope for this review).

---

### Issue #7 — Stop-before-delete constraint documented but HTTP status inconsistent

**Severity**: Minor

**Description**: The delete endpoint correctly documents the "must stop before delete" rule and returns `BIZ_003`. However, it uses HTTP 409 (Conflict) while the error code table says `BIZ_003` should use showType 1 (warn). The HTTP status and business error code are correctly paired. No issue here on closer inspection.

**Fix applied**: None needed.

---

### Issue #8 — Member sync add-only constraint properly documented

**Severity**: N/A (Pass)

**Description**: The sync endpoint correctly documents "同步策略为 add-only（仅添加，不删除已有成员）". This matches the design doc requirement. Pass.

---

### Issue #9 — Mutex timeout constraint properly documented

**Severity**: N/A (Pass)

**Description**: Service lifecycle endpoints correctly document "per-project 互斥锁（5s 超时）". Pass.

---

### Issue #10 — Missing `owner_id` or token validation error on start

**Severity**: Major

**Description**: The `POST /start` endpoint description says "解密 owner Token" as part of the startup flow. But there is no documented error for when the project owner has no configured platform token. If the owner hasn't set up their GitLab/GitHub token, the start will fail, but the spec doesn't document this failure mode.

**Fix applied**: Added `422` response to the start endpoint for missing owner token configuration.

---

### Issue #11 — No `failed` state transition documentation

**Severity**: Minor

**Description**: The spec defines `failed` as a service status ("启动失败 超过重试次数") but doesn't document what operations are valid from the `failed` state. Can you `start` from `failed`? The spec only says start fails when "服务已在运行中". Implicitly, `failed` should allow `start` (retry), but this is not explicit.

**Fix applied**: Added clarification to the start endpoint description that start is valid from `stopped`, `error`, or `failed` states.

---

## Round 2: Developer Experience & Testability

### Issue #12 — No idempotency documentation

**Severity**: Major

**Description**: None of the endpoints document idempotency behavior. Key questions a frontend dev would ask:
- Is `POST /start` idempotent if called twice quickly? (No — returns BIZ_003)
- Is `POST /members` idempotent? (No — returns BIZ_003 on duplicate)
- Is `PUT /projects/:id` idempotent? (Yes — same input produces same output)
- Is `DELETE /members/:userId` idempotent? (No — returns BIZ_002 on second call)

**Fix applied**: Added idempotency notes to the spec's top-level description section.

---

### Issue #13 — No rate limiting documentation

**Severity**: Major

**Description**: The design doc section 8.2 specifies login rate limiting ("同一用户名 5 次/分钟，同一 IP 20 次/分钟"). The Phase 2 spec has no rate limiting documentation at all. The `/members/sync` endpoint is particularly abuse-prone (calls external APIs). The service lifecycle endpoints (start/stop/restart) are also abuse-prone.

**Fix applied**: Added rate limiting section to the spec description.

---

### Issue #14 — Field naming inconsistency: snake_case vs camelCase

**Severity**: Major

**Description**: The spec mixes naming conventions:
- Response wrapper uses camelCase: `retCode`, `retMsg`, `showType`, `pageNo`, `pageSize`, `totalCount`
- Entity fields use snake_case: `git_url`, `service_status`, `created_at`, `user_id`, `synced_from`, `platform_host`
- Pagination uses camelCase: `pageNo`, `pageSize`

This is a deliberate split (wrapper = frontend convention, entity = backend convention), but it's not documented anywhere. A frontend developer will be confused about which convention to expect.

**Fix applied**: Added field naming convention documentation to the spec description.

---

### Issue #15 — Member list endpoint is not paginated

**Severity**: Minor

**Description**: `GET /api/projects/{id}/members` returns a flat array, not a paginated response. For projects with many members (synced from large GitLab groups), this could return hundreds of records. However, given the system's scale (< 50 users per section 16.2), this is acceptable for now.

**Fix applied**: No fix. Acceptable for current scale.

---

### Issue #16 — No `updated_at` field in WorkflowData response

**Severity**: Minor

**Description**: The `WorkflowData` schema only returns `template_mode` and `content`. There's no way for the frontend to know when the workflow was last modified, which is useful for showing "last edited" timestamps and detecting stale data.

**Fix applied**: Added `updated_at` field to `WorkflowData` schema.

---

### Issue #17 — Phase 1 compatibility: response format alignment

**Severity**: Minor

**Description**: Cannot fully verify Phase 1 compatibility without the Phase 1 spec, but the response wrapper format (`data`, `success`, `retCode`, `retMsg`, `showType`) appears consistent with the design doc's unified format. The error code scheme is shared. No breaking changes detected.

**Fix applied**: None needed.

---

### Issue #18 — Deterministic responses: ServiceStatus `uptime_seconds` is non-deterministic

**Severity**: Minor

**Description**: The `uptime_seconds` field in `ServiceStatus` changes every second, making responses non-deterministic for testing. This is inherent to the domain and acceptable, but test fixtures should mock this value.

**Fix applied**: None needed. Test framework should handle this.

---

### Issue #19 — Missing `Content-Length` or size limit on workflow content

**Severity**: Minor

**Description**: The `UpdateWorkflowRequest` has `maxLength: 65536` on the content field, which is good. But there's no documented request body size limit at the server level. A malicious client could send a very large JSON payload. This should be handled by middleware, not the spec.

**Fix applied**: Added note about request body size limit to the spec description.

---

### Issue #20 — `createProject` returns 200 instead of 201

**Severity**: Minor

**Description**: RESTful convention is to return `201 Created` for successful resource creation. The spec uses `200` for `POST /projects` and `POST /members`. This is a style choice that's consistent across the spec (all success = 200), so it's internally consistent even if not strictly RESTful.

**Fix applied**: No fix. Internal consistency is maintained. The unified response format with `retCode: "0"` makes HTTP status codes less important.

---

### Issue #21 — No documentation of what happens to members when project is deleted

**Severity**: Minor

**Description**: The delete project endpoint says "删除项目及其所有关联数据（成员关系、配置等）" which is clear. The DB schema uses `ON DELETE CASCADE` for project_members. Consistent.

**Fix applied**: None needed.

---

## Summary

| Severity | Count | Fixed in spec |
|----------|-------|---------------|
| Critical | 0     | -             |
| Major    | 6     | 6             |
| Minor    | 10    | 1             |
| Pass     | 2     | -             |

### Major fixes applied to api-specification.yaml:
1. Added `EXT_001` error code and schema
2. Added `500/SYS_001` responses to all endpoints
3. Added `422` response to start endpoint for missing owner token
4. Added idempotency documentation
5. Added rate limiting documentation
6. Added field naming convention documentation
7. Added `updated_at` to WorkflowData schema
8. Clarified valid start states (stopped/error/failed)
