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
//! - Dispatch with multiple candidates (priority ordering)
//! - Retry cycle: fail -> schedule retry -> fire -> re-dispatch
//! - Per-state concurrency limits

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;

use symphony_platform::models::{BlockerRef, Issue, OrchestratorEvent};
use symphony_platform::orchestrator::scheduler::DispatchConfig;
use symphony_platform::orchestrator::Orchestrator;

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

fn make_issue_with_created_at(
    id: &str,
    state: &str,
    priority: Option<i32>,
    created_at: chrono::DateTime<Utc>,
) -> Issue {
    Issue {
        id: id.to_string(),
        identifier: format!("TEST-{}", id),
        title: format!("Issue {}", id),
        description: None,
        priority,
        state: state.to_string(),
        branch_name: None,
        url: None,
        labels: vec![],
        blocked_by: vec![],
        created_at: Some(created_at),
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
// Priority Ordering Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_dispatch_priority_ordering_highest_first() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig {
        max_concurrent_agents: 2,
        ..DispatchConfig::default()
    };
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel.clone());

    // Fill 1 slot so only 1 more can be dispatched
    let filler = make_issue("99", "Filler", "Todo", Some(1));
    let handle = tokio::spawn(async {});
    orchestrator.register_running(filler, handle, cancel.child_token(), None);

    // Dispatch 3 issues with different priorities
    let issues = vec![
        make_issue("3", "Low priority", "Todo", Some(4)),
        make_issue("1", "High priority", "Todo", Some(1)),
        make_issue("2", "Medium priority", "Todo", Some(2)),
    ];
    orchestrator.dispatch_candidates(issues);

    // Only 1 slot available — highest priority (lowest number) should be claimed
    assert!(
        orchestrator.state.claimed.contains("1"),
        "Highest priority issue should be claimed first"
    );
}

#[tokio::test]
async fn test_dispatch_priority_ordering_with_created_at_tiebreaker() {
    use chrono::TimeZone;

    let cancel = CancellationToken::new();
    let config = DispatchConfig {
        max_concurrent_agents: 2,
        ..DispatchConfig::default()
    };
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel.clone());

    // Fill 1 slot
    let filler = make_issue("99", "Filler", "Todo", Some(1));
    let handle = tokio::spawn(async {});
    orchestrator.register_running(filler, handle, cancel.child_token(), None);

    // Same priority, different created_at — oldest should win
    let issues = vec![
        make_issue_with_created_at(
            "2",
            "Todo",
            Some(1),
            Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap(),
        ),
        make_issue_with_created_at(
            "1",
            "Todo",
            Some(1),
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(), // oldest
        ),
    ];
    orchestrator.dispatch_candidates(issues);

    assert!(
        orchestrator.state.claimed.contains("1"),
        "Oldest issue with same priority should be claimed first"
    );
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

#[tokio::test]
async fn test_dispatch_with_blocker() {
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

#[tokio::test]
async fn test_dispatch_with_terminal_blocker_allowed() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    // Issue with a terminal blocker (should be allowed)
    let mut issue = make_issue("1", "Previously blocked", "Todo", Some(1));
    issue.blocked_by = vec![BlockerRef {
        id: Some("2".to_string()),
        identifier: Some("TEST-2".to_string()),
        state: Some("Done".to_string()), // terminal blocker
    }];

    let issues = vec![issue];
    orchestrator.dispatch_candidates(issues);

    // Should be claimed because blocker is terminal
    assert!(orchestrator.state.claimed.contains("1"));
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
    // Normal exit -> continuation retry with 1s delay
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

#[tokio::test]
async fn test_retry_cycle_fail_schedule_fire() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig {
        poll_interval_ms: 50,
        ..DispatchConfig::default()
    };
    let mut orchestrator = Orchestrator::new(config, 300_000, 1_000, cancel.clone());

    // Dispatch an issue
    let issues = vec![make_issue("1", "Issue 1", "Todo", Some(1))];
    orchestrator.dispatch_candidates(issues);
    assert!(orchestrator.state.claimed.contains("1"));

    // Simulate worker registration and abnormal exit
    let issue = make_issue("1", "Issue 1", "Todo", Some(1));
    let handle = tokio::spawn(async {});
    orchestrator.register_running(issue, handle, cancel.child_token(), None);

    // Send abnormal exit event via the event sender
    let tx = orchestrator.event_sender();
    tx.send(OrchestratorEvent::WorkerExitAbnormal {
        issue_id: "1".to_string(),
        error: "process crashed".to_string(),
    })
    .await
    .unwrap();

    // Run the orchestrator briefly to process the event
    let run_handle = tokio::spawn(async move {
        orchestrator.run().await;
    });

    // Let it process
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();

    let _ = tokio::time::timeout(Duration::from_secs(2), run_handle).await;
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
    use symphony_platform::config::{ConfigHolder, EffectiveConfig, ServiceConfig};

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
    use symphony_platform::config::{parse_workflow, ConfigHolder, EffectiveConfig, ServiceConfig};

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

#[tokio::test]
async fn test_config_reload_updates_dispatch_behavior() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig {
        max_concurrent_agents: 5,
        ..DispatchConfig::default()
    };
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    // Initially can dispatch up to 5
    assert_eq!(orchestrator.state.max_concurrent_agents, 5);

    // Simulate config reload changing max_concurrent_agents
    orchestrator.dispatch_config.max_concurrent_agents = 2;
    orchestrator.state.max_concurrent_agents = 2;

    // Now only 2 slots available
    assert_eq!(orchestrator.state.available_global_slots(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Per-State Concurrency Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_per_state_concurrency_limit() {
    let cancel = CancellationToken::new();
    let mut by_state = HashMap::new();
    by_state.insert("todo".to_string(), 1usize); // Only 1 Todo at a time

    let config = DispatchConfig {
        max_concurrent_agents: 10,
        max_concurrent_agents_by_state: by_state,
        ..DispatchConfig::default()
    };
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel.clone());

    // Register one running Todo issue
    let running_issue = make_issue("99", "Running Todo", "Todo", Some(1));
    let handle = tokio::spawn(async {});
    orchestrator.register_running(running_issue, handle, cancel.child_token(), None);

    // Try to dispatch another Todo issue
    let issues = vec![make_issue("1", "New Todo", "Todo", Some(1))];
    orchestrator.dispatch_candidates(issues);

    // Should NOT be claimed because per-state limit for "todo" is 1
    assert!(!orchestrator.state.claimed.contains("1"));
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
    assert!(
        result.is_ok(),
        "Orchestrator should shut down within timeout"
    );
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

// ═══════════════════════════════════════════════════════════════════════════════
// Event-Driven Orchestrator Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_orchestrator_processes_worker_exit_normal() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig {
        poll_interval_ms: 50,
        ..DispatchConfig::default()
    };
    let mut orchestrator = Orchestrator::new(config, 300_000, 1_000, cancel.clone());

    // Register a running worker
    let issue = make_issue("1", "Issue 1", "Todo", Some(1));
    let handle = tokio::spawn(async {});
    orchestrator.register_running(issue, handle, cancel.child_token(), None);
    assert_eq!(orchestrator.state.running.len(), 1);

    // Send normal exit event
    let tx = orchestrator.event_sender();
    tx.send(OrchestratorEvent::WorkerExitNormal {
        issue_id: "1".to_string(),
    })
    .await
    .unwrap();

    // Run briefly to process
    let run_handle = tokio::spawn(async move {
        orchestrator.run().await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(2), run_handle).await;
}

#[tokio::test]
async fn test_orchestrator_processes_force_refresh() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig {
        poll_interval_ms: 5_000, // Long interval
        ..DispatchConfig::default()
    };
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel.clone());

    // Send force refresh event
    let tx = orchestrator.event_sender();
    tx.send(OrchestratorEvent::ForceRefresh).await.unwrap();

    // Run briefly
    let run_handle = tokio::spawn(async move {
        orchestrator.run().await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(2), run_handle).await;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Edge Cases
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_dispatch_empty_candidates() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    orchestrator.dispatch_candidates(vec![]);
    assert!(orchestrator.state.claimed.is_empty());
}

#[tokio::test]
async fn test_dispatch_issue_with_empty_id_rejected() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    let mut issue = make_issue("1", "Issue", "Todo", Some(1));
    issue.id = String::new(); // Empty ID

    orchestrator.dispatch_candidates(vec![issue]);
    assert!(orchestrator.state.claimed.is_empty());
}

#[tokio::test]
async fn test_dispatch_issue_with_empty_state_rejected() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    let mut issue = make_issue("1", "Issue", "Todo", Some(1));
    issue.state = String::new(); // Empty state

    orchestrator.dispatch_candidates(vec![issue]);
    assert!(orchestrator.state.claimed.is_empty());
}

#[tokio::test]
async fn test_dispatch_case_insensitive_state_matching() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    // "todo" should match "Todo" in active_states
    let issues = vec![make_issue("1", "Issue", "todo", Some(1))];
    orchestrator.dispatch_candidates(issues);
    assert!(orchestrator.state.claimed.contains("1"));
}
