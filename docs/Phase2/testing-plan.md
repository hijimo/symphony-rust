# Phase 2 Testing Plan - Comprehensive

## Overview

This document defines the complete testing strategy for Phase 2 (Project Management) of the Symphony Web Platform backend. The framework covers 133 test cases across 4 layers: unit, integration, API, and E2E.

For full implementation details including code, see `backend-test-framework.md`.

---

## Test Architecture

```
Layer 4: E2E Tests (6 tests)
  Full business flows across multiple endpoints
  Sequential execution, per-test database

Layer 3: API Tests (55 tests)
  One test per endpoint per scenario
  Real HTTP server, per-test database
  Tests auth, validation, permissions, business rules

Layer 2: Integration Tests (29 tests)
  Repository CRUD against real SQLite
  Auth middleware with project permissions
  Process manager with mock spawner

Layer 1: Unit Tests (43 tests)
  Pure logic: parsing, rendering, state machines
  No I/O, no database, no network
```

---

## Test Categories

### 1. Unit Tests (43 tests)

| Module | Tests | What's Validated |
|--------|-------|-----------------|
| Git URL Parser | 22 | HTTPS/SSH parsing, platform detection, normalization, edge cases, error handling |
| Workflow Template | 6 | Template rendering with variable substitution, unknown template error |
| PID Verification | 4 | Nonexistent PID, zero PID, wrong process name, time mismatch |
| Service Status State Machine | 11 | Valid/invalid transitions between all states |

### 2. Integration Tests (29 tests)

| Module | Tests | What's Validated |
|--------|-------|-----------------|
| Project Repository | 9 | CRUD, duplicate URL, admin vs user listing, cascading deletes |
| Member Repository | 8 | Add/remove/update role, duplicate detection, sync |
| Auth + Project Permissions | 5 | Owner/member/non-member/admin access patterns |
| Process Manager (mocked) | 7 | Start/stop/restart, mutex, spawn failure handling |

### 3. API Tests (55 tests)

| Endpoint Group | Tests | Scenarios Covered |
|----------------|-------|-------------------|
| POST /api/projects | 7 | Happy path, full options, no token, expired token, missing field, invalid URL, duplicate |
| GET /api/projects | 6 | User filtering, admin sees all, pagination, platform filter, search, no token |
| GET /api/projects/:id | 3 | Happy path, not found, non-member forbidden |
| PUT /api/projects/:id | 3 | Owner success, member forbidden, admin success |
| DELETE /api/projects/:id | 4 | Owner success, running service conflict, member forbidden, not found |
| POST /api/projects/:id/start | 5 | Happy path, already running, member forbidden, no token, not found |
| POST /api/projects/:id/stop | 3 | Happy path, already stopped idempotent, member forbidden |
| POST /api/projects/:id/restart | 3 | Happy path, cold start, member forbidden |
| GET /api/projects/:id/status | 4 | Stopped state, running state, member can view, non-member forbidden |
| GET /api/projects/:id/members | 3 | Happy path, non-member forbidden, no token |
| POST /api/projects/:id/members | 5 | Happy path, duplicate, nonexistent user, member forbidden, invalid role |
| PUT /api/projects/:id/members/:userId | 3 | Happy path, non-member target, member forbidden |
| DELETE /api/projects/:id/members/:userId | 3 | Happy path, last owner blocked, member forbidden |
| POST /api/projects/:id/members/sync | 3 | Happy path, no token configured, member forbidden |
| GET /api/projects/:id/workflow | 3 | Default template, non-member forbidden, no token |
| PUT /api/projects/:id/workflow | 4 | Custom content, empty content, member forbidden, missing fields |
| POST /api/projects/:id/workflow/reset | 3 | Reset success, member forbidden, no token |

### 4. E2E Tests (6 tests)

| Flow | Tests | What's Validated |
|------|-------|-----------------|
| Project Lifecycle | 2 | Create -> configure -> start -> health check -> stop -> delete; Cannot delete running |
| Member Management | 2 | Add -> role change -> cascading permissions -> remove -> access revoked; Visibility isolation |
| Concurrent Operations | 2 | Simultaneous start/stop mutex; Concurrent creation unique constraint |

---

## Permission Matrix (tested per endpoint)

| Endpoint | Admin | Owner | Member | Non-Member | No Token |
|----------|-------|-------|--------|------------|----------|
| GET /api/projects | All | Own | Own | None | 401 |
| POST /api/projects | 200 | 200 | 200 | 200 | 401 |
| GET /api/projects/:id | 200 | 200 | 200 | 403 | 401 |
| PUT /api/projects/:id | 200 | 200 | 403 | 403 | 401 |
| DELETE /api/projects/:id | 200 | 200 | 403 | 403 | 401 |
| POST .../start | 200 | 200 | 403 | 403 | 401 |
| POST .../stop | 200 | 200 | 403 | 403 | 401 |
| POST .../restart | 200 | 200 | 403 | 403 | 401 |
| GET .../status | 200 | 200 | 200 | 403 | 401 |
| GET .../members | 200 | 200 | 200 | 403 | 401 |
| POST .../members | 200 | 200 | 403 | 403 | 401 |
| PUT .../members/:uid | 200 | 200 | 403 | 403 | 401 |
| DELETE .../members/:uid | 200 | 200 | 403 | 403 | 401 |
| POST .../members/sync | 200 | 200 | 403 | 403 | 401 |
| GET .../workflow | 200 | 200 | 200 | 403 | 401 |
| PUT .../workflow | 200 | 200 | 403 | 403 | 401 |
| POST .../workflow/reset | 200 | 200 | 403 | 403 | 401 |

---

## Test Infrastructure

### Key Components

1. **TestApp** - Spins up a real Axum server with in-memory SQLite per test
2. **Fixtures** - Predefined Git URLs, user credentials, request payloads
3. **MockProcessSpawner** - Records spawn/kill operations without real subprocesses
4. **MockPidVerifier** - Configurable PID validation for deterministic tests
5. **Token helpers** - Generate valid, expired, and role-specific JWT tokens

### Dependencies (dev-dependencies in Cargo.toml)

```toml
[dev-dependencies]
tokio-test = "0.4"
tempfile = "3"
reqwest = { version = "0.12", features = ["json"] }
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
```

---

## CI/CD Integration

Tests run in GitLab CI at: `http://gitlab.jushuitan-inc.com:8081/zimei10525/symphony_e2e_test_repo`

### Pipeline Stages

1. **lint** - clippy + rustfmt (parallel)
2. **test** - unit / integration / API / E2E (parallel jobs)
3. **report** - coverage via cargo-tarpaulin

### Execution Strategy

| Job | Parallelism | Reason |
|-----|-------------|--------|
| Unit tests | `--test-threads=auto` | Pure logic, no shared state |
| Integration tests | `--test-threads=1` | Shared DB within test file |
| API tests | `--test-threads=4` | Per-test server isolation |
| E2E tests | `--test-threads=1` | Multi-step flows, ordering matters |

---

## Running Tests Locally

```bash
# All tests
cargo test -p web-platform

# By category
cargo test -p web-platform --lib                    # Unit
cargo test -p web-platform --test 'repo_*'          # Integration: repos
cargo test -p web-platform --test 'api_*'           # API
cargo test -p web-platform --test 'e2e*'            # E2E

# Single test
cargo test -p web-platform create_project_happy_path

# With coverage
cargo tarpaulin -p web-platform --out html
```

---

## Success Criteria

- All 133 tests pass in CI
- Code coverage > 80% for Phase 2 modules
- No flaky tests (deterministic with mocks)
- API tests cover every endpoint with every permission level
- E2E tests validate complete business workflows
- CI pipeline completes in < 5 minutes
