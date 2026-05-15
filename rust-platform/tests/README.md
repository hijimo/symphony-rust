# Symphony Platform Adapter — Test Guide

## Test Profiles

### Unit Tests

Run the library's internal unit tests:

```bash
cargo test --lib
```

These test individual modules in isolation (config parsing, workflow loading,
memory adapter, cooldown queue, etc.) and require no external dependencies.

### Integration Tests

Run all integration tests (excludes `#[ignore]` tests):

```bash
cargo test --test '*'
```

Or run specific test files:

```bash
cargo test --test orchestrator_test
cargo test --test config_validator_test
cargo test --test fault_injection_test
cargo test --test retry_test
```

### E2E Tests

Run the full end-to-end test suite:

```bash
cargo test --test e2e_lifecycle
cargo test --test e2e_config_reload
cargo test --test e2e_shutdown
cargo test --test e2e_http_server
```

Or all E2E tests at once:

```bash
cargo test --test 'e2e_*'
```

E2E tests use the `MemoryAdapter` and `FakeCodexProcess` to simulate the full
orchestration lifecycle without network calls. They validate:

- Issue dispatch and lifecycle management
- Configuration hot-reload behavior
- Graceful shutdown under various conditions
- HTTP server API contracts

### Real Integration Tests

Run tests against real external APIs (requires credentials):

```bash
# Linear API tests
LINEAR_API_KEY=lin_api_xxx cargo test --test real_linear_smoke -- --ignored

# GitHub API tests
GITHUB_PERSONAL_ACCESS_TOKEN_SYMPHONEY=ghp_xxx cargo test --test real_platform_smoke -- --ignored

# All real integration tests
LINEAR_API_KEY=lin_api_xxx GITHUB_PERSONAL_ACCESS_TOKEN_SYMPHONEY=ghp_xxx cargo test -- --ignored
```

## Required Environment Variables

| Variable | Profile | Description |
|----------|---------|-------------|
| `LINEAR_API_KEY` | Real Integration | Linear API key with read access |
| `LINEAR_TEST_PROJECT` | Real Integration | (Optional) Linear project slug |
| `GITHUB_PERSONAL_ACCESS_TOKEN_SYMPHONEY` | Real Integration | GitHub token with repo access |
| `GITHUB_TEST_OWNER` | Real Integration | (Optional) GitHub org/user |
| `GITHUB_TEST_REPO` | Real Integration | (Optional) Repository name |

## Test Isolation

- **Unit tests**: Fully isolated, no shared state
- **Integration tests**: Use `MemoryAdapter` for platform operations
- **E2E tests**: Use `tempfile` for workspace isolation, `MemoryAdapter` for platform
- **Real integration**: Use isolated test identifiers; clean up artifacts when practical

## Test Structure

```
tests/
├── common/mod.rs              # Shared test helpers and fixtures
├── e2e/
│   └── harness/
│       ├── mod.rs             # Harness module root
│       ├── fake_codex.rs      # FakeCodexProcess simulator
│       ├── fake_linear.rs     # FakeLinearServer (wiremock)
│       └── test_orchestrator.rs # TestOrchestrator wrapper
├── fixtures/
│   ├── valid_workflow.md      # Complete valid WORKFLOW.md
│   ├── minimal_workflow.md    # Only required fields
│   ├── invalid_yaml_workflow.md # Broken YAML for error tests
│   ├── no_frontmatter_workflow.md # Prompt only
│   └── custom_hooks_workflow.md # All hooks defined
├── e2e_lifecycle.rs           # Full lifecycle E2E tests
├── e2e_config_reload.rs       # Config reload E2E tests
├── e2e_shutdown.rs            # Graceful shutdown E2E tests
├── e2e_http_server.rs         # HTTP server E2E tests
├── real_linear_smoke.rs       # Linear API smoke tests (#[ignore])
├── real_platform_smoke.rs     # GitHub API smoke tests (#[ignore])
├── e2e_test.rs                # Legacy E2E test (MemoryAdapter workflow)
├── orchestrator_test.rs       # Orchestrator unit integration
├── config_validator_test.rs   # Config validation tests
├── fault_injection_test.rs    # Fault injection tests
├── retry_test.rs              # Retry logic tests
└── github_adapter_test.rs     # GitHub adapter tests
```

## CI Integration

### Standard CI (no credentials needed)

```yaml
- name: Run tests
  run: |
    cargo test --lib
    cargo test --test 'e2e_*'
    cargo test --test orchestrator_test
    cargo test --test config_validator_test
```

### Integration CI (with credentials)

```yaml
- name: Run real integration tests
  env:
    LINEAR_API_KEY: ${{ secrets.LINEAR_API_KEY }}
    GITHUB_PERSONAL_ACCESS_TOKEN_SYMPHONEY: ${{ secrets.GITHUB_TOKEN }}
  run: |
    cargo test -- --ignored
```

### Compile Check (fastest feedback)

```bash
cargo test --no-run
```

## Writing New Tests

### E2E Test Pattern

```rust
#[tokio::test]
async fn e2e_my_new_scenario() {
    std::env::set_var("FAKE_TOKEN", "test-token");

    let adapter = Arc::new(MemoryAdapter::new());
    let cancel = CancellationToken::new();

    // Seed test data
    adapter.seed_issue(make_test_issue(1, "Test", Some("workflow::todo"))).await;

    // Build orchestrator
    let config = Arc::new(test_config(100));
    let cooldown = Arc::new(CooldownQueue::new(Duration::from_millis(100)));
    cooldown.spawn_cleanup_task(cancel.clone());
    let mut orchestrator = Orchestrator::new(adapter.clone(), cooldown, config, cancel.clone());

    // Execute
    orchestrator.poll_cycle().await.unwrap();

    // Assert
    assert_eq!(orchestrator.dispatched().len(), 1);
}
```

### Real Integration Test Pattern

```rust
#[tokio::test]
#[ignore]  // REQUIRED: marks as opt-in
async fn real_my_api_test() {
    let api_key = std::env::var("MY_API_KEY")
        .expect("MY_API_KEY required for this test");

    // Test against real API
    // ...
}
```

## Troubleshooting

- **Tests hang**: Check for missing `cancel.cancel()` in shutdown tests
- **Flaky timing**: Increase `Duration` values in `tokio::time::sleep`
- **Real integration fails**: Verify credentials and network access
- **Compile errors in harness**: Run `cargo test --no-run` to check
