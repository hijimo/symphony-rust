//! Unit tests for retry computation logic.
//!
//! Tests cover:
//! - Exponential backoff formula: min(10000 * 2^(attempt-1), max_backoff)
//! - Continuation retry (fixed 1000ms)
//! - Backoff cap at max_backoff_ms
//! - Edge cases: zero attempt, very large attempt (no overflow)
//! - Retry scheduling and timer management
//! - Claim release

use tokio::sync::mpsc;

use symphony_platform::models::{OrchestratorState, RetryKind};
use symphony_platform::orchestrator::retry::{
    compute_retry_delay, release_claim, schedule_retry, RetrySchedule,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Continuation Retry Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod continuation_retry {
    use super::*;

    #[test]
    fn test_continuation_always_1000ms_attempt_1() {
        assert_eq!(
            compute_retry_delay(1, &RetryKind::Continuation, 300_000),
            1_000
        );
    }

    #[test]
    fn test_continuation_always_1000ms_attempt_5() {
        assert_eq!(
            compute_retry_delay(5, &RetryKind::Continuation, 300_000),
            1_000
        );
    }

    #[test]
    fn test_continuation_always_1000ms_attempt_100() {
        assert_eq!(
            compute_retry_delay(100, &RetryKind::Continuation, 300_000),
            1_000
        );
    }

    #[test]
    fn test_continuation_ignores_max_backoff() {
        // Even with a very low max_backoff, continuation is always 1000ms
        assert_eq!(compute_retry_delay(1, &RetryKind::Continuation, 500), 1_000);
    }

    #[test]
    fn test_continuation_attempt_zero() {
        // Edge case: attempt 0 should still return 1000ms
        assert_eq!(
            compute_retry_delay(0, &RetryKind::Continuation, 300_000),
            1_000
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Exponential Backoff Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod exponential_backoff {
    use super::*;

    #[test]
    fn test_failure_attempt_1_is_10s() {
        // 10000 * 2^0 = 10000
        assert_eq!(compute_retry_delay(1, &RetryKind::Failure, 300_000), 10_000);
    }

    #[test]
    fn test_failure_attempt_2_is_20s() {
        // 10000 * 2^1 = 20000
        assert_eq!(compute_retry_delay(2, &RetryKind::Failure, 300_000), 20_000);
    }

    #[test]
    fn test_failure_attempt_3_is_40s() {
        // 10000 * 2^2 = 40000
        assert_eq!(compute_retry_delay(3, &RetryKind::Failure, 300_000), 40_000);
    }

    #[test]
    fn test_failure_attempt_4_is_80s() {
        // 10000 * 2^3 = 80000
        assert_eq!(compute_retry_delay(4, &RetryKind::Failure, 300_000), 80_000);
    }

    #[test]
    fn test_failure_attempt_5_is_160s() {
        // 10000 * 2^4 = 160000
        assert_eq!(
            compute_retry_delay(5, &RetryKind::Failure, 300_000),
            160_000
        );
    }

    #[test]
    fn test_failure_attempt_6_capped_at_300s() {
        // 10000 * 2^5 = 320000 -> capped at 300000
        assert_eq!(
            compute_retry_delay(6, &RetryKind::Failure, 300_000),
            300_000
        );
    }

    #[test]
    fn test_failure_attempt_10_capped() {
        // 10000 * 2^9 = 5120000 -> capped at 300000
        assert_eq!(
            compute_retry_delay(10, &RetryKind::Failure, 300_000),
            300_000
        );
    }

    #[test]
    fn test_backoff_with_custom_cap() {
        let max_backoff = 60_000; // 60 seconds cap

        assert_eq!(
            compute_retry_delay(1, &RetryKind::Failure, max_backoff),
            10_000
        );
        assert_eq!(
            compute_retry_delay(2, &RetryKind::Failure, max_backoff),
            20_000
        );
        assert_eq!(
            compute_retry_delay(3, &RetryKind::Failure, max_backoff),
            40_000
        );
        // attempt 4: 80000 -> capped at 60000
        assert_eq!(
            compute_retry_delay(4, &RetryKind::Failure, max_backoff),
            60_000
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Edge Cases
// ═══════════════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn test_failure_attempt_zero_no_panic() {
        // saturating_sub(1) on 0 gives 0, so 10000 * 2^0 = 10000
        let delay = compute_retry_delay(0, &RetryKind::Failure, 300_000);
        assert_eq!(delay, 10_000);
    }

    #[test]
    fn test_failure_very_large_attempt_no_overflow() {
        // Very large attempt should not overflow due to saturating_mul/saturating_pow
        let delay = compute_retry_delay(100, &RetryKind::Failure, 300_000);
        assert_eq!(delay, 300_000); // Should be capped
    }

    #[test]
    fn test_failure_u32_max_attempt_no_overflow() {
        let delay = compute_retry_delay(u32::MAX, &RetryKind::Failure, 300_000);
        assert_eq!(delay, 300_000); // Should be capped, not overflow
    }

    #[test]
    fn test_max_backoff_zero() {
        // If max_backoff is 0, all failure delays should be 0
        assert_eq!(compute_retry_delay(1, &RetryKind::Failure, 0), 0);
    }

    #[test]
    fn test_max_backoff_very_large() {
        // With a very large cap, the formula should compute normally
        let delay = compute_retry_delay(5, &RetryKind::Failure, u64::MAX);
        assert_eq!(delay, 160_000); // 10000 * 2^4
    }

    #[test]
    fn test_backoff_at_power_10() {
        // attempt 11: 10000 * 2^10 = 10240000 (10240s = ~2.8 hours)
        // With default cap of 300000, this is capped
        assert_eq!(
            compute_retry_delay(11, &RetryKind::Failure, 300_000),
            300_000
        );

        // Without cap, verify the formula
        let uncapped = compute_retry_delay(11, &RetryKind::Failure, u64::MAX);
        assert_eq!(uncapped, 10_000 * 1024); // 10000 * 2^10 = 10240000
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Retry Scheduling Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod retry_scheduling {
    use super::*;

    #[tokio::test]
    async fn test_schedule_retry_creates_entry_and_claims() {
        let (tx, _rx) = mpsc::channel(16);
        let mut state = OrchestratorState::new(30_000, 10);

        schedule_retry(
            &mut state,
            RetrySchedule::new(
                "issue-1",
                "TEST-1",
                1,
                RetryKind::Failure,
                10_000,
                Some("test error".to_string()),
            ),
            &tx,
        );

        assert!(state.retry_attempts.contains_key("issue-1"));
        assert!(state.claimed.contains("issue-1"));

        let entry = state.retry_attempts.get("issue-1").unwrap();
        assert_eq!(entry.attempt, 1);
        assert_eq!(entry.retry_kind, RetryKind::Failure);
        assert_eq!(entry.error, Some("test error".to_string()));
        assert_eq!(entry.identifier, "TEST-1");

        // Cleanup
        entry.timer_handle.abort();
    }

    #[tokio::test]
    async fn test_schedule_retry_cancels_existing_timer() {
        let (tx, _rx) = mpsc::channel(16);
        let mut state = OrchestratorState::new(30_000, 10);

        // Schedule first retry with long delay
        schedule_retry(
            &mut state,
            RetrySchedule::new("issue-1", "TEST-1", 1, RetryKind::Failure, 60_000, None),
            &tx,
        );

        // Schedule second retry for same issue (should cancel first)
        schedule_retry(
            &mut state,
            RetrySchedule::new(
                "issue-1",
                "TEST-1",
                2,
                RetryKind::Failure,
                20_000,
                Some("new error".to_string()),
            ),
            &tx,
        );

        // Only one entry should exist
        assert_eq!(state.retry_attempts.len(), 1);
        let entry = state.retry_attempts.get("issue-1").unwrap();
        assert_eq!(entry.attempt, 2);
        assert_eq!(entry.error, Some("new error".to_string()));

        // Cleanup
        entry.timer_handle.abort();
    }

    #[tokio::test]
    async fn test_schedule_continuation_retry() {
        let (tx, _rx) = mpsc::channel(16);
        let mut state = OrchestratorState::new(30_000, 10);

        schedule_retry(
            &mut state,
            RetrySchedule::new("issue-1", "TEST-1", 1, RetryKind::Continuation, 1_000, None),
            &tx,
        );

        let entry = state.retry_attempts.get("issue-1").unwrap();
        assert_eq!(entry.retry_kind, RetryKind::Continuation);
        assert_eq!(entry.error, None);

        // Cleanup
        entry.timer_handle.abort();
    }

    #[tokio::test]
    async fn test_release_claim_removes_entry_and_unclaims() {
        let (tx, _rx) = mpsc::channel(16);
        let mut state = OrchestratorState::new(30_000, 10);

        schedule_retry(
            &mut state,
            RetrySchedule::new("issue-1", "TEST-1", 1, RetryKind::Failure, 60_000, None),
            &tx,
        );

        assert!(state.claimed.contains("issue-1"));
        assert!(state.retry_attempts.contains_key("issue-1"));

        release_claim(&mut state, "issue-1");

        assert!(!state.claimed.contains("issue-1"));
        assert!(!state.retry_attempts.contains_key("issue-1"));
    }

    #[tokio::test]
    async fn test_release_claim_nonexistent_is_noop() {
        let mut state = OrchestratorState::new(30_000, 10);

        // Should not panic
        release_claim(&mut state, "nonexistent");
        assert!(!state.claimed.contains("nonexistent"));
    }

    #[tokio::test]
    async fn test_retry_timer_fires_event() {
        let (tx, mut rx) = mpsc::channel(16);
        let mut state = OrchestratorState::new(30_000, 10);

        // Schedule with very short delay
        schedule_retry(
            &mut state,
            RetrySchedule::new("issue-1", "TEST-1", 1, RetryKind::Failure, 10, None),
            &tx,
        );

        // Wait for the timer to fire
        let event = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
            .await
            .unwrap()
            .unwrap();

        // Verify it's a RetryFired event
        match event {
            symphony_platform::models::OrchestratorEvent::RetryFired { issue_id } => {
                assert_eq!(issue_id, "issue-1");
            }
            _ => panic!("Expected RetryFired event"),
        }
    }
}
