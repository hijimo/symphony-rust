# API Specification - Review Fixes Applied

**Date**: 2026-05-21  
**Based on**: Round 1 + Round 2 adversarial reviews

## Fixes Applied

### C1 + C2: Response format alignment
- `showType` is omitted on success responses (matches existing `skip_serializing_if`)
- `retMsg` uses `"ok"` on success (matches existing implementation)

### C3: POST /issues returns 200
- Kept as 200 to match existing Phase 1/2 convention (create_project also returns 200)
- Documented as accepted deviation from REST 201 convention

### C4: SSE frontend integration guide added
- Documented fetch + ReadableStream approach
- AbortController for cancellation
- Auth handling for SSE

### C5: Cache invalidation on write
- POST /issues invalidates kanban cache for that user+project
- Documented in cache strategy section

### M1: Field naming convention resolved
- Decision: Entity fields use snake_case (preserving GitLab/GitHub API field names)
- Explicitly documented as intentional divergence from Phase 1/2 camelCase
- Frontend uses a transform layer (already has `caseTransform.ts`)

### M2: showType per-error-code
- Documented as prerequisite refactoring for Phase 3
- Implementation will add showType mapping to error.rs

### M10: Token-invalid vs service-down distinction
- New error: `TOKEN_001` for "平台 Token 无效或已过期"
- `EXT_001` reserved for actual service unavailability

### M8: Global AI rate limit
- Added system-wide limit: 30 AI calls/minute (configurable)
- Per-user limit remains at 10/minute

### M9: SSE keepalive
- Server sends `: keepalive\n\n` every 15s
- Max generation time: 120s
- Azure OpenAI timeout: 30s for first token

### Additional fixes from Round 2
- Added `platform` field to KanbanData
- Added `Retry-After` header to 429 responses
- Documented concurrent AI generation limit (1 per user)
- Added note about `iid` = GitLab iid / GitHub number
