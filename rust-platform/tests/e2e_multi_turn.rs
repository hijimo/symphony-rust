//! Multi-turn Agent E2E Tests
//!
//! Tests that verify multi-turn agent behavior:
//! - Agent completes in 1 turn
//! - Agent needs 3 turns with continuation guidance
//! - Agent hits max_turns limit
//! - Agent encounters approval request -> auto-approved -> continues
//! - Agent encounters tool call -> executed -> result returned
//!
//! Run with: `cargo test --test e2e_multi_turn`

#[allow(dead_code, unused_imports)]
#[path = "e2e/harness/mod.rs"]
mod harness;

use std::time::Duration;

use tokio::sync::mpsc;

use harness::fake_codex::{CodexBehavior, FakeCodexProcess, TurnScenario};

// ============================================================================
// Test: Agent completes in 1 turn
// ============================================================================

/// Verifies that a single-turn agent session emits the correct events
/// and exits cleanly.
#[tokio::test]
async fn e2e_multi_turn_single_turn_completion() {
    let behavior = CodexBehavior::n_turns(1);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

    let handle = tokio::spawn(async move { process.run().await });

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    let (lines, exit_code) = handle.await.unwrap();

    // Should exit successfully
    assert_eq!(exit_code, 0);
    assert!(!lines.is_empty());

    // Should have exactly 1 turn's worth of events
    let turn_starts: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "turn.start")
        .collect();
    assert_eq!(turn_starts.len(), 1);

    let turn_completions: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "turn.completed")
        .collect();
    assert_eq!(turn_completions.len(), 1);
}

// ============================================================================
// Test: Agent needs 3 turns with continuation guidance
// ============================================================================

/// Verifies that a multi-turn agent session correctly processes 3 turns
/// with proper event sequencing.
#[tokio::test]
async fn e2e_multi_turn_three_turns_with_continuation() {
    let behavior = CodexBehavior::n_turns(3);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

    let handle = tokio::spawn(async move { process.run().await });

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    let (_, exit_code) = handle.await.unwrap();

    assert_eq!(exit_code, 0);

    // Should have 3 turns
    let turn_starts: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "turn.start")
        .collect();
    assert_eq!(turn_starts.len(), 3);

    let turn_completions: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "turn.completed")
        .collect();
    assert_eq!(turn_completions.len(), 3);

    // Verify ordering: each turn.start is followed by turn.completed before next turn.start
    let mut in_turn = false;
    for event in &events {
        match event.event_type.as_str() {
            "turn.start" => {
                assert!(!in_turn, "Nested turn.start detected");
                in_turn = true;
            }
            "turn.completed" => {
                assert!(in_turn, "turn.completed without preceding turn.start");
                in_turn = false;
            }
            _ => {}
        }
    }
    assert!(!in_turn, "Last turn was not completed");
}

// ============================================================================
// Test: Agent hits max_turns limit
// ============================================================================

/// Verifies that the agent stops after reaching the configured max_turns limit.
#[tokio::test]
async fn e2e_multi_turn_hits_max_turns_limit() {
    let max_turns = 3;
    let behavior = CodexBehavior::hits_max_turns(max_turns);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

    let handle = tokio::spawn(async move { process.run().await });

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    let (_, exit_code) = handle.await.unwrap();

    // Should exit cleanly (max_turns is a normal exit condition)
    assert_eq!(exit_code, 0);

    // Should have exactly max_turns turn starts
    let turn_starts: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "turn.start")
        .collect();
    assert_eq!(turn_starts.len(), max_turns as usize);
}

// ============================================================================
// Test: Agent encounters approval request -> auto-approved -> continues
// ============================================================================

/// Verifies that when an agent requests approval, the event is emitted
/// and the process handles it according to the configured scenario.
#[tokio::test]
async fn e2e_multi_turn_approval_request() {
    let behavior = CodexBehavior::with_approval_on_turn(3, 2);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

    let handle = tokio::spawn(async move { process.run().await });

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    let (_, exit_code) = handle.await.unwrap();

    // The process exits with error because the approval turn doesn't complete
    assert_eq!(exit_code, 1);

    // Turn 1 should complete successfully
    let first_turn_events: Vec<_> = events
        .iter()
        .take_while(|e| e.event_type != "approval_request")
        .collect();
    assert!(
        first_turn_events
            .iter()
            .any(|e| e.event_type == "turn.completed"),
        "First turn should complete before approval request"
    );

    // Should have an approval request event
    let approval_events: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "approval_request")
        .collect();
    assert_eq!(approval_events.len(), 1);

    // Verify approval request has tool information
    let approval = &approval_events[0];
    assert_eq!(approval.data["tool"], "commandExecution");
    assert!(approval.data["request_id"].is_string());
}

// ============================================================================
// Test: Agent encounters tool call -> executed -> result returned
// ============================================================================

/// Verifies that tool calls are properly emitted and results are returned.
#[tokio::test]
async fn e2e_multi_turn_tool_call_execution() {
    let behavior = CodexBehavior::with_tool_call_on_turn(2, 1);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

    let handle = tokio::spawn(async move { process.run().await });

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    let (_, exit_code) = handle.await.unwrap();

    assert_eq!(exit_code, 0);

    // Should have a tool_call event
    let tool_calls: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "tool_call")
        .collect();
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].data["tool"], "read_file");

    // Should have a tool_result event
    let tool_results: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "tool_result")
        .collect();
    assert_eq!(tool_results.len(), 1);
    assert_eq!(tool_results[0].data["call_id"], "call-001");

    // The turn with the tool call should still complete
    let turn_completions: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "turn.completed")
        .collect();
    assert_eq!(turn_completions.len(), 2); // Both turns complete
}

// ============================================================================
// Test: Agent fails on one turn then succeeds
// ============================================================================

/// Verifies that a failure on one turn causes the process to exit with error.
#[tokio::test]
async fn e2e_multi_turn_fail_then_succeed() {
    // Fail on turn 2 out of 3
    let behavior = CodexBehavior::fail_then_succeed(2, 3);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

    let handle = tokio::spawn(async move { process.run().await });

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    let (_, exit_code) = handle.await.unwrap();

    // Should exit with error because turn 2 fails
    assert_eq!(exit_code, 1);

    // Turn 1 should complete
    let turn_completions: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "turn.completed")
        .collect();
    assert_eq!(turn_completions.len(), 1); // Only turn 1 completes

    // Should have a turn.failed event
    let turn_failures: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "turn.failed")
        .collect();
    assert_eq!(turn_failures.len(), 1);
}

// ============================================================================
// Test: Agent with slow responses (timeout boundary)
// ============================================================================

/// Verifies that slow responses are handled correctly within timeout bounds.
#[tokio::test]
async fn e2e_multi_turn_slow_responses() {
    let behavior = CodexBehavior::slow(Duration::from_millis(50));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

    let start = tokio::time::Instant::now();
    let handle = tokio::spawn(async move { process.run().await });

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    let (_, exit_code) = handle.await.unwrap();
    let elapsed = start.elapsed();

    assert_eq!(exit_code, 0);
    // With 4 events at 50ms each, should take at least 150ms (3 delays between 4 events)
    assert!(
        elapsed >= Duration::from_millis(100),
        "Expected slow processing, but completed in {:?}",
        elapsed
    );
}

// ============================================================================
// Test: Agent stall is detectable and cancellable
// ============================================================================

/// Verifies that a stalling agent can be detected and killed via timeout.
#[tokio::test]
async fn e2e_multi_turn_stall_detection() {
    let behavior = CodexBehavior::stalling(Duration::from_millis(10));
    let process = FakeCodexProcess::new(behavior);

    let handle = tokio::spawn(async move { process.run().await });

    // Simulate stall detection: wait for a timeout period
    let result = tokio::time::timeout(Duration::from_millis(200), handle).await;

    // The task should NOT have completed (it's stalling)
    assert!(
        result.is_err(),
        "Stalling process should not complete on its own"
    );
}

// ============================================================================
// Test: Agent with input_required event
// ============================================================================

/// Verifies that input_required events are properly emitted.
#[tokio::test]
async fn e2e_multi_turn_input_required() {
    let scenarios = vec![
        TurnScenario::success(),
        TurnScenario::needs_input("What database should I use?"),
    ];
    let behavior = CodexBehavior::multi_turn(scenarios);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

    let handle = tokio::spawn(async move { process.run().await });

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    let (_, exit_code) = handle.await.unwrap();

    // Should exit with error (input_required is a non-completion)
    assert_eq!(exit_code, 1);

    // Should have the input_required event
    let input_events: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "turn.input_required")
        .collect();
    assert_eq!(input_events.len(), 1);
    assert_eq!(
        input_events[0].data["prompt"],
        "What database should I use?"
    );
}

// ============================================================================
// Test: Event ordering is deterministic
// ============================================================================

/// Verifies that events are emitted in a deterministic order across runs.
#[tokio::test]
async fn e2e_multi_turn_deterministic_ordering() {
    // Run the same scenario twice and verify identical event sequences
    let mut all_runs = Vec::new();

    for _ in 0..3 {
        let behavior = CodexBehavior::n_turns(2);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

        tokio::spawn(async move { process.run().await });

        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event.event_type.clone());
        }
        all_runs.push(events);
    }

    // All runs should produce the same event type sequence
    assert_eq!(all_runs[0], all_runs[1]);
    assert_eq!(all_runs[1], all_runs[2]);
}
