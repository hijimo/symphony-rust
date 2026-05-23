#![allow(
    unused_imports,
    unused_variables,
    dead_code,
    clippy::bind_instead_of_map,
    clippy::derivable_impls,
    clippy::manual_range_contains,
    clippy::needless_borrows_for_generic_args,
    clippy::ptr_arg,
    clippy::duplicated_attributes,
    clippy::approx_constant,
    clippy::bool_assert_comparison,
    clippy::len_zero,
    clippy::let_and_return
)]

//! Orchestrator integration tests using the event-driven API.
//!
//! These tests verify the orchestrator's dispatch logic, shutdown behavior,
//! and state management using the actual DispatchConfig-based API.

use std::collections::HashMap;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use symphony_platform::models::Issue;
use symphony_platform::orchestrator::scheduler::DispatchConfig;
use symphony_platform::orchestrator::Orchestrator;

/// Helper to create a test issue (models::Issue).
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

/// Test: dispatch_candidates claims eligible issues.
///
/// Seeds issues with active workflow states and verifies they are claimed
/// after calling dispatch_candidates.
#[tokio::test]
async fn test_dispatch_candidates_claims_todo_issues() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    let issues = vec![
        make_issue("1", "Todo issue", "Todo", Some(1)),
        make_issue("2", "In progress", "In Progress", Some(2)),
        make_issue("3", "Done issue", "Done", Some(3)),
    ];

    orchestrator.dispatch_candidates(issues);

    // Issues 1 and 2 are in active states, issue 3 is terminal
    assert!(orchestrator.state.claimed.contains("1"));
    assert!(orchestrator.state.claimed.contains("2"));
    assert!(!orchestrator.state.claimed.contains("3"));
}

/// Test: Already-claimed issues are not re-dispatched.
///
/// If an issue is already in the claimed set, it should not be claimed again.
#[tokio::test]
async fn test_already_claimed_not_re_dispatched() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    // Pre-claim issue 1
    orchestrator.state.claimed.insert("1".to_string());

    let issues = vec![
        make_issue("1", "Already claimed", "Todo", Some(1)),
        make_issue("2", "New issue", "Todo", Some(2)),
    ];

    orchestrator.dispatch_candidates(issues);

    // Issue 1 was already claimed, issue 2 should be newly claimed
    assert!(orchestrator.state.claimed.contains("1"));
    assert!(orchestrator.state.claimed.contains("2"));
    assert_eq!(orchestrator.state.claimed.len(), 2);
}

/// Test: Graceful shutdown — orchestrator exits when cancel signal is sent.
///
/// The orchestrator should stop its event loop and exit cleanly when the
/// CancellationToken is cancelled.
#[tokio::test]
async fn test_graceful_shutdown() {
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    let config = DispatchConfig {
        poll_interval_ms: 50,
        ..DispatchConfig::default()
    };
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    // Spawn orchestrator in background
    let handle = tokio::spawn(async move {
        orchestrator.run().await;
    });

    // Give it a moment to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Send cancel signal
    cancel_clone.cancel();

    // Orchestrator should exit within a reasonable time
    let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
    assert!(
        result.is_ok(),
        "Orchestrator should shut down within 2 seconds of cancel"
    );
    assert!(
        result.unwrap().is_ok(),
        "Orchestrator task should not panic"
    );
}

/// Test: Dispatch skipped when shutting_down flag is set.
#[tokio::test]
async fn test_dispatch_skipped_when_shutting_down() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    // Set shutting_down
    orchestrator.state.shutting_down = true;

    let issues = vec![make_issue("1", "Should not dispatch", "Todo", Some(1))];
    orchestrator.dispatch_candidates(issues);

    assert!(!orchestrator.state.claimed.contains("1"));
}

/// Test: Concurrency limit is respected.
///
/// When max_concurrent_agents is set and running is at capacity, no new issues are claimed.
#[tokio::test]
async fn test_concurrency_limit_respected() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig {
        max_concurrent_agents: 2,
        ..DispatchConfig::default()
    };
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel.clone());

    // Fill both slots with running entries
    for i in 10..=11 {
        let issue = make_issue(&i.to_string(), &format!("Filler {}", i), "Todo", Some(1));
        let handle = tokio::spawn(async {});
        orchestrator.register_running(issue, handle, cancel.child_token(), None);
    }

    let issues: Vec<Issue> = (1..=5)
        .map(|i| make_issue(&i.to_string(), &format!("Issue {}", i), "Todo", Some(1)))
        .collect();

    orchestrator.dispatch_candidates(issues);

    // No new claims since running is at capacity
    assert_eq!(orchestrator.state.claimed.len(), 0);
    assert_eq!(orchestrator.state.running.len(), 2);
}

/// Test: release_issue_claim removes from claimed set.
#[tokio::test]
async fn test_release_issue_claim() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    let issues = vec![make_issue("1", "Test", "Todo", Some(1))];
    orchestrator.dispatch_candidates(issues);
    assert!(orchestrator.state.claimed.contains("1"));

    orchestrator.release_issue_claim("1");
    assert!(!orchestrator.state.claimed.contains("1"));
}

/// Test: event_sender returns a working sender.
#[tokio::test]
async fn test_event_sender() {
    let cancel = CancellationToken::new();
    let config = DispatchConfig::default();
    let orchestrator = Orchestrator::new(config, 300_000, 300_000, cancel);

    let sender = orchestrator.event_sender();
    // Should be able to send an event without error
    let result = sender
        .send(symphony_platform::models::OrchestratorEvent::ForceRefresh)
        .await;
    assert!(result.is_ok());
}
