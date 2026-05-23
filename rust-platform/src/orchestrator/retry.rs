//! Retry queue — exponential backoff and continuation retry scheduling.
//!
//! Implements SPEC section 8.4: retry entry creation, backoff formula,
//! timer management, and claim release.

use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::models::{
    current_monotonic_ms, OrchestratorEvent, OrchestratorState, RetryEntry, RetryKind,
};

/// Compute the retry delay in milliseconds (SPEC section 8.4).
///
/// - Continuation retries (normal exit): fixed 1000ms delay.
/// - Failure retries (abnormal exit): 10000 * 2^(attempt-1), capped at max_backoff_ms.
pub fn compute_retry_delay(attempt: u32, retry_kind: &RetryKind, max_backoff_ms: u64) -> u64 {
    match retry_kind {
        RetryKind::Continuation => 1_000, // Fixed 1s for continuation
        RetryKind::Failure => {
            // Exponential backoff: 10s * 2^(attempt-1), capped
            let exponent = attempt.saturating_sub(1);
            let delay = 10_000u64.saturating_mul(2u64.saturating_pow(exponent));
            delay.min(max_backoff_ms)
        }
    }
}

/// Schedule a retry for an issue (SPEC section 8.4).
///
/// SPEC requirement: MUST cancel any existing retry timer for the same issue
/// before creating a new one.
#[allow(clippy::too_many_arguments)]
pub fn schedule_retry(
    state: &mut OrchestratorState,
    issue_id: &str,
    identifier: &str,
    attempt: u32,
    retry_kind: RetryKind,
    delay_ms: u64,
    error: Option<String>,
    event_tx: &mpsc::Sender<OrchestratorEvent>,
) {
    // Cancel existing retry timer for the same issue (SPEC section 8.4 MUST)
    if let Some(existing) = state.retry_attempts.remove(issue_id) {
        existing.timer_handle.abort();
        tracing::debug!(
            issue_id,
            old_attempt = existing.attempt,
            "cancelled existing retry timer"
        );
    }

    let due_at_ms = current_monotonic_ms() + delay_ms;
    let tx = event_tx.clone();
    let id = issue_id.to_string();

    let timer_handle: JoinHandle<()> = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        let _ = tx
            .send(OrchestratorEvent::RetryFired { issue_id: id })
            .await;
    });

    tracing::info!(
        issue_id,
        identifier,
        attempt,
        delay_ms,
        retry_kind = ?retry_kind,
        error = error.as_deref().unwrap_or("none"),
        "scheduled retry"
    );

    state.retry_attempts.insert(
        issue_id.to_string(),
        RetryEntry {
            issue_id: issue_id.to_string(),
            identifier: identifier.to_string(),
            attempt,
            retry_kind,
            due_at_ms,
            timer_handle,
            error,
        },
    );

    // Ensure issue stays in claimed set
    state.claimed.insert(issue_id.to_string());
}

/// Release a claim: remove retry entry, abort timer, and unclaim the issue.
/// (SPEC section 8.4)
pub fn release_claim(state: &mut OrchestratorState, issue_id: &str) {
    if let Some(entry) = state.retry_attempts.remove(issue_id) {
        entry.timer_handle.abort();
        tracing::info!(
            issue_id,
            identifier = %entry.identifier,
            "released claim, aborted retry timer"
        );
    }
    state.claimed.remove(issue_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_retry_delay_continuation() {
        assert_eq!(
            compute_retry_delay(1, &RetryKind::Continuation, 300_000),
            1_000
        );
        assert_eq!(
            compute_retry_delay(5, &RetryKind::Continuation, 300_000),
            1_000
        );
    }

    #[test]
    fn test_compute_retry_delay_failure_exponential() {
        // attempt 1: 10000 * 2^0 = 10000
        assert_eq!(compute_retry_delay(1, &RetryKind::Failure, 300_000), 10_000);
        // attempt 2: 10000 * 2^1 = 20000
        assert_eq!(compute_retry_delay(2, &RetryKind::Failure, 300_000), 20_000);
        // attempt 3: 10000 * 2^2 = 40000
        assert_eq!(compute_retry_delay(3, &RetryKind::Failure, 300_000), 40_000);
        // attempt 4: 10000 * 2^3 = 80000
        assert_eq!(compute_retry_delay(4, &RetryKind::Failure, 300_000), 80_000);
        // attempt 5: 10000 * 2^4 = 160000
        assert_eq!(
            compute_retry_delay(5, &RetryKind::Failure, 300_000),
            160_000
        );
        // attempt 6: 10000 * 2^5 = 320000 -> capped at 300000
        assert_eq!(
            compute_retry_delay(6, &RetryKind::Failure, 300_000),
            300_000
        );
    }

    #[test]
    fn test_compute_retry_delay_failure_cap() {
        // Very high attempt should be capped
        assert_eq!(
            compute_retry_delay(100, &RetryKind::Failure, 300_000),
            300_000
        );
    }

    #[tokio::test]
    async fn test_schedule_retry_creates_entry() {
        let (tx, _rx) = mpsc::channel(16);
        let mut state = OrchestratorState::new(30_000, 10);

        schedule_retry(
            &mut state,
            "issue-1",
            "TEST-1",
            1,
            RetryKind::Failure,
            10_000,
            Some("test error".to_string()),
            &tx,
        );

        assert!(state.retry_attempts.contains_key("issue-1"));
        assert!(state.claimed.contains("issue-1"));

        let entry = state.retry_attempts.get("issue-1").unwrap();
        assert_eq!(entry.attempt, 1);
        assert_eq!(entry.retry_kind, RetryKind::Failure);
        assert_eq!(entry.error, Some("test error".to_string()));

        // Cleanup
        entry.timer_handle.abort();
    }

    #[tokio::test]
    async fn test_schedule_retry_cancels_existing() {
        let (tx, _rx) = mpsc::channel(16);
        let mut state = OrchestratorState::new(30_000, 10);

        // Schedule first retry
        schedule_retry(
            &mut state,
            "issue-1",
            "TEST-1",
            1,
            RetryKind::Failure,
            60_000,
            None,
            &tx,
        );

        // Schedule second retry for same issue (should cancel first)
        schedule_retry(
            &mut state,
            "issue-1",
            "TEST-1",
            2,
            RetryKind::Failure,
            20_000,
            Some("new error".to_string()),
            &tx,
        );

        // Only one entry should exist
        assert_eq!(state.retry_attempts.len(), 1);
        let entry = state.retry_attempts.get("issue-1").unwrap();
        assert_eq!(entry.attempt, 2);

        // Cleanup
        entry.timer_handle.abort();
    }

    #[tokio::test]
    async fn test_release_claim() {
        let (tx, _rx) = mpsc::channel(16);
        let mut state = OrchestratorState::new(30_000, 10);

        schedule_retry(
            &mut state,
            "issue-1",
            "TEST-1",
            1,
            RetryKind::Failure,
            60_000,
            None,
            &tx,
        );

        assert!(state.claimed.contains("issue-1"));
        assert!(state.retry_attempts.contains_key("issue-1"));

        release_claim(&mut state, "issue-1");

        assert!(!state.claimed.contains("issue-1"));
        assert!(!state.retry_attempts.contains_key("issue-1"));
    }
}
