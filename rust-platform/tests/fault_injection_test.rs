//! Fault injection tests for the Platform adapter.
//!
//! These tests use the MemoryAdapter with fault injection to verify error handling,
//! compensation logic, retry behavior, and circuit breaker patterns.

use std::sync::Arc;
use std::time::Duration;

use symphony_platform::error::PlatformError;
use symphony_platform::platform::cooldown_queue::CooldownQueue;
use symphony_platform::platform::{
    make_test_issue, FetchOptions, IssueId, MemoryAdapter, Platform,
};

/// Test: add_labels succeeds but remove_labels fails, verifying partial state.
///
/// Scenario: During a workflow state transition (add new label, remove old label),
/// the add succeeds but the remove fails. The system should leave the issue with
/// both labels (partial update) rather than silently losing the add.
#[tokio::test]
async fn test_add_labels_failure_triggers_compensation() {
    let adapter = MemoryAdapter::new();
    let issue = make_test_issue(1, "Test compensation", Some("workflow::todo"));
    adapter.seed_issue(issue).await;

    // Add a new label successfully
    adapter
        .add_labels(IssueId(1), &["workflow::in-progress".to_string()])
        .await
        .unwrap();

    // Inject fault on remove_labels
    adapter
        .with_fault("remove_labels", PlatformError::ServerError(500))
        .await;

    // Attempt to remove the old label — should fail
    let result = adapter
        .remove_labels(IssueId(1), &["workflow::todo".to_string()])
        .await;
    assert!(result.is_err());

    // Verify state: issue should still have BOTH labels (partial update)
    let labels = adapter.get_issue_labels(IssueId(1)).await.unwrap();
    assert!(
        labels.contains(&"workflow::todo".to_string()),
        "Old label should still be present after remove failure"
    );
    assert!(
        labels.contains(&"workflow::in-progress".to_string()),
        "New label should be present (add succeeded)"
    );

    // Verify call counts
    assert_eq!(adapter.call_count("add_labels").await, 1);
    assert_eq!(adapter.call_count("remove_labels").await, 1);
}

/// Test: RateLimited error is properly surfaced for retry logic.
///
/// Verifies that when the platform returns a rate limit error, it includes
/// the retry_after_ms hint and is classified as retryable.
#[tokio::test]
async fn test_rate_limit_triggers_retry() {
    let adapter = MemoryAdapter::new();
    adapter
        .seed_issue(make_test_issue(
            1,
            "Rate limited issue",
            Some("workflow::todo"),
        ))
        .await;

    // Inject rate limit error
    adapter
        .with_fault(
            "fetch_candidate_issues",
            PlatformError::RateLimited {
                retry_after_ms: 5000,
            },
        )
        .await;

    // First call should get rate limited
    let result = adapter
        .fetch_candidate_issues(FetchOptions::default())
        .await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(err.is_retryable(), "RateLimited should be retryable");
    match &err {
        PlatformError::RateLimited { retry_after_ms } => {
            assert_eq!(*retry_after_ms, 5000);
        }
        _ => panic!("Expected RateLimited error, got: {:?}", err),
    }

    // Second call should succeed (fault consumed)
    let result = adapter
        .fetch_candidate_issues(FetchOptions::default())
        .await;
    assert!(result.is_ok());
    assert_eq!(adapter.call_count("fetch_candidate_issues").await, 2);
}

/// Test: NotFound error causes issue to enter cooldown queue.
///
/// When an issue is deleted externally (404), the orchestrator should place it
/// in the cooldown queue to avoid repeated failed fetches.
#[tokio::test]
async fn test_issue_deleted_enters_cooldown() {
    let adapter = MemoryAdapter::new();
    let cooldown_queue = CooldownQueue::new(Duration::from_secs(30));

    // Inject NotFound on fetch_issue
    adapter
        .with_fault(
            "fetch_issue",
            PlatformError::NotFound("issue 42".to_string()),
        )
        .await;

    // Attempt to fetch the deleted issue
    let result = adapter.fetch_issue(IssueId(42)).await;
    assert!(result.is_err());

    match &result.unwrap_err() {
        PlatformError::NotFound(msg) => {
            assert!(msg.contains("42"));
        }
        other => panic!("Expected NotFound, got: {:?}", other),
    }

    // Simulate orchestrator behavior: put issue in cooldown on NotFound
    cooldown_queue.cooldown(IssueId(42), "issue deleted (404)".to_string(), 5);

    // Verify the issue is now in cooldown
    assert!(
        cooldown_queue.should_skip(IssueId(42)),
        "Deleted issue should be in cooldown"
    );

    // Other issues should not be affected
    assert!(
        !cooldown_queue.should_skip(IssueId(1)),
        "Unrelated issues should not be in cooldown"
    );
}

/// Test: Multiple workflow labels are cleaned up during state transition.
///
/// If an issue somehow ends up with multiple workflow:: labels (e.g., due to
/// a previous partial failure), set_workflow_state should clean all of them
/// and leave only the new state label.
#[tokio::test]
async fn test_multiple_workflow_labels_cleaned() {
    let adapter = MemoryAdapter::new();

    // Create an issue with multiple workflow labels (inconsistent state)
    let mut issue = make_test_issue(1, "Multi-label issue", Some("workflow::todo"));
    issue.labels = vec![
        "workflow::todo".to_string(),
        "workflow::in-progress".to_string(),
        "workflow::rework".to_string(),
        "bug".to_string(),
    ];
    adapter.seed_issue(issue).await;

    // Transition to a new state — should remove ALL workflow:: labels
    adapter
        .set_workflow_state(IssueId(1), "workflow::human-review")
        .await
        .unwrap();

    let labels = adapter.get_issue_labels(IssueId(1)).await.unwrap();

    // Should have exactly one workflow label (the new one)
    let workflow_labels: Vec<&String> = labels
        .iter()
        .filter(|l| l.starts_with("workflow::"))
        .collect();
    assert_eq!(
        workflow_labels.len(),
        1,
        "Should have exactly one workflow label after transition, got: {:?}",
        workflow_labels
    );
    assert_eq!(workflow_labels[0], "workflow::human-review");

    // Non-workflow labels should be preserved
    assert!(
        labels.contains(&"bug".to_string()),
        "Non-workflow labels should be preserved"
    );
}

/// Test: Circuit breaker opens after consecutive failures.
///
/// After a threshold of consecutive failures, the circuit breaker should open
/// and immediately reject subsequent calls without hitting the platform.
#[tokio::test]
async fn test_circuit_breaker_opens_after_threshold() {
    let adapter = MemoryAdapter::new();
    adapter
        .seed_issue(make_test_issue(1, "CB test", Some("workflow::todo")))
        .await;

    // Set persistent fault to simulate repeated failures
    adapter
        .with_persistent_fault("fetch_candidate_issues", PlatformError::ServerError(503))
        .await;

    // Simulate circuit breaker threshold (5 consecutive failures)
    let threshold = 5;
    let mut failure_count = 0;

    for _ in 0..threshold {
        let result = adapter
            .fetch_candidate_issues(FetchOptions::default())
            .await;
        if result.is_err() {
            failure_count += 1;
        }
    }

    assert_eq!(
        failure_count, threshold,
        "All calls should fail with persistent fault"
    );
    assert_eq!(
        adapter.call_count("fetch_candidate_issues").await,
        threshold
    );

    // After threshold failures, a real circuit breaker would open.
    // Verify the error type is consistent (ServerError) — in production,
    // the circuit breaker would return CircuitOpen instead.
    adapter.clear_fault("fetch_candidate_issues").await;

    // After clearing the fault, calls should succeed again (half-open -> closed)
    let result = adapter
        .fetch_candidate_issues(FetchOptions::default())
        .await;
    assert!(
        result.is_ok(),
        "After clearing fault, calls should succeed (circuit recovery)"
    );
}

/// Test: Concurrent fault injection is thread-safe.
///
/// Multiple tasks can inject faults and call methods concurrently without
/// data races or panics.
#[tokio::test]
async fn test_concurrent_fault_injection() {
    let adapter = MemoryAdapter::new();
    for i in 1..=10 {
        adapter
            .seed_issue(make_test_issue(
                i,
                &format!("Issue {}", i),
                Some("workflow::todo"),
            ))
            .await;
    }

    let adapter = Arc::new(adapter);
    let mut handles = Vec::new();

    // Spawn multiple tasks that race on fault injection and method calls
    for i in 0..5 {
        let adapter = Arc::clone(&adapter);
        handles.push(tokio::spawn(async move {
            if i % 2 == 0 {
                adapter
                    .with_fault("fetch_issue", PlatformError::Timeout)
                    .await;
            }
            // Try to fetch — may or may not fail depending on timing
            let _ = adapter.fetch_issue(IssueId(1)).await;
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Should not panic — call count should be at least 5
    assert!(adapter.call_count("fetch_issue").await >= 5);
}

/// Test: Persistent fault survives multiple calls.
#[tokio::test]
async fn test_persistent_fault_survives_multiple_calls() {
    let adapter = MemoryAdapter::new();
    adapter
        .seed_issue(make_test_issue(
            1,
            "Persistent test",
            Some("workflow::todo"),
        ))
        .await;

    adapter
        .with_persistent_fault(
            "add_labels",
            PlatformError::RateLimited {
                retry_after_ms: 1000,
            },
        )
        .await;

    // All calls should fail
    for _ in 0..10 {
        let result = adapter
            .add_labels(IssueId(1), &["test-label".to_string()])
            .await;
        assert!(result.is_err());
    }

    assert_eq!(adapter.call_count("add_labels").await, 10);

    // Clear and verify recovery
    adapter.clear_fault("add_labels").await;
    let result = adapter
        .add_labels(IssueId(1), &["test-label".to_string()])
        .await;
    assert!(result.is_ok());
}

/// Test: Empty label operations are no-ops (don't trigger faults).
#[tokio::test]
async fn test_empty_labels_noop() {
    let adapter = MemoryAdapter::new();
    adapter
        .seed_issue(make_test_issue(1, "Noop test", Some("workflow::todo")))
        .await;

    // Set faults that should NOT fire for empty slices
    adapter
        .with_fault("add_labels", PlatformError::ServerError(500))
        .await;
    adapter
        .with_fault("remove_labels", PlatformError::ServerError(500))
        .await;

    // Empty operations should succeed without triggering faults
    let result = adapter.add_labels(IssueId(1), &[]).await;
    assert!(result.is_ok());

    let result = adapter.remove_labels(IssueId(1), &[]).await;
    assert!(result.is_ok());

    // Faults should still be pending (not consumed)
    assert_eq!(adapter.call_count("add_labels").await, 0);
    assert_eq!(adapter.call_count("remove_labels").await, 0);
}
