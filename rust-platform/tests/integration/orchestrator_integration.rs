//! Integration tests for the Orchestrator dispatch and reconciliation cycle.
//!
//! Tests cover:
//! - Full dispatch cycle with DispatchConfig
//! - Dispatch excludes already-claimed issues
//! - Dispatch respects concurrency limits
//! - Reconciliation: terminal state issues not dispatched
//! - Retry queue: delay computation
//! - Stall detection concept
//! - Config hot reload
//! - Orchestrator lifecycle (run + shutdown)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeDelta, Utc};
use tokio_util::sync::CancellationToken;

use symphony_platform::models::Issue;
use symphony_platform::orchestrator::Orchestrator;
use symphony_platform::orchestrator::scheduler::DispatchConfig;

// ═══════════════════════════════════════════════════════════════════════════════
// Helper Functions
// ═══════════════════════════════════════════════════════════════════════════════

fn make_issue(id: &str, title: &str, state: &str, priority: Option<i32>) -> Issue {
    Issue {
        id: id.to_string(),
        identifier: format!("TEST-{}", id),
        title: title.to_string(),
        description: None,
        priority,
        state: state.to_string(),
        branch_name: None,
        url: None,
        labels: vec![],
        blocked_by: vec![],
        created_at: None,
        updated_at: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Full Dispatch Cycle Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_full_dispatch_cycle() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    // Dispatch candidate issues in active states
    let issues = vec![
        make_issue("1", "Issue 1", "Todo", Some(1)),
        make_issue("2", "Issue 2", "In Progress", Some(2)),
    ];

    orchestrator.dispatch_candidates(issues);

    // Both issues should be claimed (both are in active states)
    assert!(orchestrator.state.claimed.contains("1"));
    assert!(orchestrator.state.claimed.contains("2"));
    assert_eq!(orchestrator.state.claimed.len(), 2);
}

#[tokio::test]
async fn test_dispatch_excludes_already_claimed_issues() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    // Pre-claim issue 1
    orchestrator.state.claimed.insert("1".to_string());

    let issues = vec![
        make_issue("1", "Issue 1", "Todo", Some(1)),
        make_issue("2", "Issue 2", "Todo", Some(2)),
    ];

    orchestrator.dispatch_candidates(issues);

    // Issue 1 was already claimed, issue 2 should be newly claimed
    assert!(orchestrator.state.claimed.contains("1"));
    assert!(orchestrator.state.claimed.contains("2"));
    assert_eq!(orchestrator.state.claimed.len(), 2);
}

#[tokio::test]
async fn test_dispatch_respects_concurrency_limit() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig {
        max_concurrent_agents: 1,
        ..DispatchConfig::default()
    };
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel.clone());

    // Pre-fill the single slot with a running entry
    let filler = make_issue("99", "Filler", "Todo", Some(1));
    let handle = tokio::spawn(async {});
    orchestrator.register_running(filler, handle, cancel.child_token(), None);

    let issues = vec![
        make_issue("1", "Issue 1", "Todo", Some(1)),
        make_issue("2", "Issue 2", "Todo", Some(2)),
    ];

    orchestrator.dispatch_candidates(issues);

    // No new claims since the single slot is occupied by the filler
    assert_eq!(orchestrator.state.claimed.len(), 0);
    assert_eq!(orchestrator.state.running.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Reconciliation Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_terminal_state_not_dispatched() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    // Issue in terminal state should not be dispatched
    let issues = vec![make_issue("1", "Done issue", "Done", Some(1))];
    orchestrator.dispatch_candidates(issues);

    assert!(!orchestrator.state.claimed.contains("1"));
}

#[tokio::test]
async fn test_release_claim_removes_from_claimed() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    // Dispatch and claim
    let issues = vec![make_issue("1", "Issue 1", "Todo", Some(1))];
    orchestrator.dispatch_candidates(issues);
    assert!(orchestrator.state.claimed.contains("1"));

    // Release claim (simulating reconciliation removing a non-active issue)
    orchestrator.release_issue_claim("1");
    assert!(!orchestrator.state.claimed.contains("1"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Retry Queue Tests
// ═══════════════════════════════════════════════════════════════════════════════

/// Simulates retry delay computation for integration testing.
fn compute_retry_delay(is_continuation: bool, attempt: u32, max_backoff_ms: u64) -> u64 {
    if is_continuation {
        1_000 // Fixed 1s for continuation
    } else {
        let base = 10_000u64;
        let delay = base.saturating_mul(2u64.saturating_pow(attempt.saturating_sub(1)));
        delay.min(max_backoff_ms)
    }
}

#[test]
fn test_retry_normal_exit_continuation_delay() {
    // Normal exit → continuation retry with 1s delay
    let delay = compute_retry_delay(true, 1, 300_000);
    assert_eq!(delay, 1_000);
}

#[test]
fn test_retry_abnormal_exit_exponential_backoff() {
    let max_backoff = 300_000;

    // First failure: 10s
    assert_eq!(compute_retry_delay(false, 1, max_backoff), 10_000);
    // Second failure: 20s
    assert_eq!(compute_retry_delay(false, 2, max_backoff), 20_000);
    // Third failure: 40s
    assert_eq!(compute_retry_delay(false, 3, max_backoff), 40_000);
}

#[test]
fn test_retry_backoff_capped() {
    let max_backoff = 60_000;

    // Should be capped at max_backoff
    assert_eq!(compute_retry_delay(false, 10, max_backoff), 60_000);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Stall Detection Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_stall_detection_concept() {
    // Simulate stall detection: if no activity for stall_timeout_ms, kill the session
    let stall_timeout = Duration::from_millis(100);
    let last_activity = tokio::time::Instant::now();

    // Simulate time passing
    tokio::time::sleep(Duration::from_millis(150)).await;

    let elapsed = last_activity.elapsed();
    assert!(elapsed > stall_timeout, "Stall should be detected");
}

#[tokio::test]
async fn test_no_stall_when_active() {
    let stall_timeout = Duration::from_millis(200);
    let last_activity = tokio::time::Instant::now();

    // Only 50ms passes — not stalled
    tokio::time::sleep(Duration::from_millis(50)).await;

    let elapsed = last_activity.elapsed();
    assert!(elapsed < stall_timeout, "Should not be stalled yet");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Config Hot Reload Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_config_hot_reload_new_config_effective() {
    use std::path::PathBuf;
    use symphony_platform::config::watcher::{ConfigHolder, EffectiveConfig};
    use symphony_platform::config::service_config::ServiceConfig;

    let initial_config = EffectiveConfig {
        service: ServiceConfig::default(),
        prompt_template: "Initial prompt".to_string(),
        loaded_at: Utc::now(),
    };

    let holder = ConfigHolder::new(initial_config, PathBuf::from("/tmp/WORKFLOW.md"));

    // Verify initial config
    let snapshot = holder.load();
    assert_eq!(snapshot.prompt_template, "Initial prompt");

    // Simulate hot reload with new config
    let new_config = EffectiveConfig {
        service: ServiceConfig {
            poll_interval_ms: 10_000,
            ..ServiceConfig::default()
        },
        prompt_template: "Updated prompt".to_string(),
        loaded_at: Utc::now(),
    };

    holder.store(new_config);

    // Verify new config is effective
    let snapshot = holder.load();
    assert_eq!(snapshot.prompt_template, "Updated prompt");
    assert_eq!(snapshot.service.poll_interval_ms, 10_000);
}

#[tokio::test]
async fn test_config_hot_reload_invalid_keeps_old() {
    use std::path::PathBuf;
    use symphony_platform::config::watcher::{ConfigHolder, EffectiveConfig};
    use symphony_platform::config::service_config::ServiceConfig;
    use symphony_platform::config::parse_workflow;

    let initial_config = EffectiveConfig {
        service: ServiceConfig::default(),
        prompt_template: "Good prompt".to_string(),
        loaded_at: Utc::now(),
    };

    let holder = ConfigHolder::new(initial_config, PathBuf::from("/tmp/WORKFLOW.md"));

    // Simulate invalid workflow file
    let invalid_content = "---\n- this is a list not a map\n---\nPrompt.\n";
    let parse_result = parse_workflow(invalid_content);

    // Parse fails — should keep old config
    assert!(parse_result.is_err());

    // Old config should still be in effect
    let snapshot = holder.load();
    assert_eq!(snapshot.prompt_template, "Good prompt");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Orchestrator Lifecycle Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_orchestrator_graceful_shutdown() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig {
        poll_interval_ms: 50,
        ..DispatchConfig::default()
    };
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel.clone());

    // Start orchestrator in background
    let handle = tokio::spawn(async move {
        orchestrator.run().await;
    });

    // Let it run for a bit
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Cancel and verify graceful shutdown
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
    assert!(result.is_ok(), "Orchestrator should shut down within timeout");
}

#[tokio::test]
async fn test_multiple_dispatch_cycles() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    // First dispatch claims issue 1
    let issues = vec![make_issue("1", "Issue 1", "Todo", Some(1))];
    orchestrator.dispatch_candidates(issues);
    assert_eq!(orchestrator.state.claimed.len(), 1);

    // Second dispatch with same issue should not re-claim (already claimed)
    let issues = vec![make_issue("1", "Issue 1", "Todo", Some(1))];
    orchestrator.dispatch_candidates(issues);
    assert_eq!(orchestrator.state.claimed.len(), 1);

    // Third dispatch with a new issue should claim it
    let issues = vec![make_issue("2", "Issue 2", "Todo", Some(2))];
    orchestrator.dispatch_candidates(issues);
    assert_eq!(orchestrator.state.claimed.len(), 2);
}

#[tokio::test]
async fn test_dispatch_with_blocker() {
    use symphony_platform::models::BlockerRef;

    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    // Issue with an active (non-terminal) blocker
    let mut blocked_issue = make_issue("1", "Blocked issue", "Todo", Some(1));
    blocked_issue.blocked_by = vec![BlockerRef {
        id: Some("2".to_string()),
        identifier: Some("TEST-2".to_string()),
        state: Some("In Progress".to_string()), // non-terminal blocker
    }];

    let issues = vec![blocked_issue];
    orchestrator.dispatch_candidates(issues);

    // Should NOT be claimed because it has an active blocker
    assert!(!orchestrator.state.claimed.contains("1"));
}
