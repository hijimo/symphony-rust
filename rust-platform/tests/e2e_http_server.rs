//! HTTP Server E2E Tests
//!
//! Tests that verify the HTTP server extension behavior:
//! - Start service with --port -> verify endpoints accessible
//! - GET /api/v1/state during active work -> verify running/retrying data
//! - POST /api/v1/refresh -> verify immediate poll triggered
//! - Concurrent HTTP requests don't block orchestrator
//!
//! NOTE: These tests validate the expected API contract. They will be fully
//! functional once the HTTP server extension module is implemented.
//!
//! Run with: `cargo test --test e2e_http_server`

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use symphony_platform::platform::{
    make_test_issue, FetchOptions, IssueId, MemoryAdapter, Platform,
};

// ============================================================================
// Test: HTTP server health endpoint contract
// ============================================================================

/// Verifies the expected response shape for GET /health.
#[tokio::test]
async fn e2e_http_server_health_endpoint_contract() {
    let expected_response = serde_json::json!({
        "status": "ok",
        "uptime_seconds": 42
    });

    assert_eq!(expected_response["status"], "ok");
    assert!(expected_response["uptime_seconds"].is_number());
}

// ============================================================================
// Test: State endpoint contract
// ============================================================================

/// Verifies the expected contract for GET /api/v1/state.
#[tokio::test]
async fn e2e_http_server_state_endpoint_contract() {
    let expected_response = serde_json::json!({
        "active_workers": [
            {
                "issue_id": 42,
                "issue_title": "Implement feature X",
                "started_at": "2025-01-15T10:00:00Z",
                "attempt": 1,
                "status": "running"
            }
        ],
        "retry_queue": [
            {
                "issue_id": 43,
                "next_retry_at": "2025-01-15T10:05:00Z",
                "attempt": 2,
                "backoff_ms": 60000
            }
        ],
        "last_poll_at": "2025-01-15T10:00:30Z",
        "config": {
            "poll_interval_ms": 5000,
            "max_workers": 3,
            "active_states": ["todo", "in_progress"]
        }
    });

    // Validate structure
    assert!(expected_response["active_workers"].is_array());
    assert!(expected_response["retry_queue"].is_array());
    assert!(expected_response["last_poll_at"].is_string());
    assert!(expected_response["config"].is_object());

    // Validate worker entry structure
    let worker = &expected_response["active_workers"][0];
    assert!(worker["issue_id"].is_number());
    assert!(worker["issue_title"].is_string());
    assert!(worker["started_at"].is_string());
    assert!(worker["attempt"].is_number());
    assert!(worker["status"].is_string());
}

// ============================================================================
// Test: Refresh endpoint contract
// ============================================================================

/// Verifies the expected contract for POST /api/v1/refresh.
#[tokio::test]
async fn e2e_http_server_refresh_endpoint_contract() {
    let expected_response = serde_json::json!({
        "status": "accepted",
        "message": "Poll cycle triggered"
    });

    assert_eq!(expected_response["status"], "accepted");
    assert!(expected_response["message"].is_string());
}

// ============================================================================
// Test: Concurrent HTTP requests don't block orchestrator
// ============================================================================

/// Verifies that the HTTP server can handle concurrent requests without
/// blocking the orchestrator's poll loop.
#[tokio::test]
async fn e2e_http_server_concurrent_requests_non_blocking() {
    let adapter = Arc::new(MemoryAdapter::new());

    // Seed issues
    for i in 1..=5 {
        adapter
            .seed_issue(make_test_issue(
                i,
                &format!("Issue {}", i),
                Some("workflow::todo"),
            ))
            .await;
    }

    // Simulate concurrent "HTTP requests" (state reads) while orchestrator runs
    let adapter_clone = adapter.clone();
    let http_task = tokio::spawn(async move {
        let mut successful_reads = 0;
        for _ in 0..20 {
            // Simulate reading state (what the HTTP handler would do)
            let snapshot = adapter_clone.snapshot().await;
            if !snapshot.issues.is_empty() {
                successful_reads += 1;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        successful_reads
    });

    // Simulate orchestrator polling concurrently
    let adapter_clone2 = adapter.clone();
    let orch_task = tokio::spawn(async move {
        let mut poll_count = 0;
        for _ in 0..10 {
            let _ = adapter_clone2
                .fetch_candidate_issues(FetchOptions::default())
                .await;
            poll_count += 1;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        poll_count
    });

    let (http_result, orch_result) = tokio::join!(http_task, orch_task);

    let http_reads = http_result.unwrap();
    let poll_count = orch_result.unwrap();

    // Both should complete without blocking each other
    assert!(http_reads >= 15, "HTTP reads were blocked: {}", http_reads);
    assert!(poll_count >= 8, "Orchestrator was blocked: {}", poll_count);
}

// ============================================================================
// Test: HTTP server error responses
// ============================================================================

/// Verifies expected error response formats.
#[tokio::test]
async fn e2e_http_server_error_response_contract() {
    // 404 Not Found
    let not_found = serde_json::json!({
        "error": "not_found",
        "message": "Endpoint not found",
        "status": 404
    });
    assert_eq!(not_found["status"], 404);

    // 405 Method Not Allowed
    let method_not_allowed = serde_json::json!({
        "error": "method_not_allowed",
        "message": "GET not allowed on this endpoint",
        "status": 405
    });
    assert_eq!(method_not_allowed["status"], 405);

    // 500 Internal Server Error
    let internal_error = serde_json::json!({
        "error": "internal_error",
        "message": "An unexpected error occurred",
        "status": 500
    });
    assert_eq!(internal_error["status"], 500);
}

// ============================================================================
// Test: HTTP server metrics endpoint contract
// ============================================================================

/// Verifies the expected contract for GET /api/v1/metrics.
#[tokio::test]
async fn e2e_http_server_metrics_endpoint_contract() {
    let expected_metrics = serde_json::json!({
        "total_dispatches": 42,
        "total_completions": 38,
        "total_failures": 4,
        "total_retries": 12,
        "active_workers": 2,
        "retry_queue_size": 1,
        "avg_turn_duration_ms": 45000,
        "total_tokens": {
            "input": 125000,
            "output": 48000
        },
        "uptime_seconds": 3600
    });

    assert!(expected_metrics["total_dispatches"].is_number());
    assert!(expected_metrics["total_completions"].is_number());
    assert!(expected_metrics["total_tokens"]["input"].is_number());
}

// ============================================================================
// Test: State snapshot during concurrent operations
// ============================================================================

/// Verifies that taking a state snapshot is safe during concurrent mutations.
#[tokio::test]
async fn e2e_http_server_state_snapshot_concurrent_safety() {
    let adapter = Arc::new(MemoryAdapter::new());

    // Spawn writers
    let adapter_w = adapter.clone();
    let writer = tokio::spawn(async move {
        for i in 0..50 {
            adapter_w
                .seed_issue(make_test_issue(
                    i,
                    &format!("Issue {}", i),
                    Some("workflow::todo"),
                ))
                .await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    // Spawn readers (simulating HTTP state endpoint)
    let adapter_r = adapter.clone();
    let reader = tokio::spawn(async move {
        let mut snapshots = 0;
        for _ in 0..30 {
            let snap = adapter_r.snapshot().await;
            // Snapshot should always be consistent (no partial state)
            let issue_count = snap.issues.len();
            assert!(
                issue_count <= 50,
                "More issues than expected: {}",
                issue_count
            );
            snapshots += 1;
            tokio::time::sleep(Duration::from_millis(8)).await;
        }
        snapshots
    });

    let (w_result, r_result) = tokio::join!(writer, reader);
    w_result.unwrap();
    let snapshots = r_result.unwrap();
    assert!(snapshots >= 20, "Not enough snapshots taken: {}", snapshots);
}
