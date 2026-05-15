//! Cooldown queue unit tests.
//!
//! Tests the DashMap-based cooldown queue that prevents repeatedly processing
//! issues that have recently failed. Issues enter cooldown for N polling cycles
//! and are skipped until the cooldown expires.

mod common;

use chrono::{TimeDelta, Utc};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;

// =============================================================================
// CooldownQueue implementation (mirrors production code)
// =============================================================================

/// Platform-native issue ID (mirrors production IssueId).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct IssueId(pub u64);

/// Entry in the cooldown queue tracking when an issue can be retried.
struct CooldownEntry {
    #[allow(dead_code)]
    reason: String,
    skip_until: chrono::DateTime<Utc>,
}

/// Concurrent-safe cooldown queue using DashMap.
///
/// Issues that fail processing are placed in cooldown for a configurable
/// number of polling cycles. The queue is checked before dispatching each issue.
struct CooldownQueue {
    entries: Arc<DashMap<IssueId, CooldownEntry>>,
    polling_interval: Duration,
}

impl CooldownQueue {
    fn new(polling_interval: Duration) -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
            polling_interval,
        }
    }

    /// Check if an issue should be skipped (is in active cooldown).
    fn should_skip(&self, issue_id: IssueId) -> bool {
        self.entries
            .get(&issue_id)
            .map(|entry| Utc::now() < entry.skip_until)
            .unwrap_or(false)
    }

    /// Place an issue into cooldown for the specified number of polling cycles.
    fn cooldown(&self, issue_id: IssueId, reason: String, cycles: u32) {
        let cooldown_duration = self.polling_interval.saturating_mul(cycles);
        let skip_until =
            Utc::now() + TimeDelta::seconds(cooldown_duration.as_secs() as i64);
        self.entries
            .insert(issue_id, CooldownEntry { reason, skip_until });
    }

    /// Place an issue into cooldown with a specific expiry time (for testing).
    fn cooldown_until(
        &self,
        issue_id: IssueId,
        reason: String,
        skip_until: chrono::DateTime<Utc>,
    ) {
        self.entries
            .insert(issue_id, CooldownEntry { reason, skip_until });
    }

    /// Remove expired entries from the queue.
    fn cleanup_expired(&self) {
        let now = Utc::now();
        self.entries.retain(|_, entry| now < entry.skip_until);
    }

    /// Get the number of entries currently in the queue.
    fn len(&self) -> usize {
        self.entries.len()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[tokio::test]
async fn test_should_skip_returns_false_for_unknown_issue() {
    let queue = CooldownQueue::new(Duration::from_secs(30));

    // An issue that was never added to the cooldown queue should not be skipped
    let issue_id = IssueId(42);
    assert!(
        !queue.should_skip(issue_id),
        "Unknown issue should not be skipped"
    );
}

#[tokio::test]
async fn test_cooldown_then_skip() {
    let queue = CooldownQueue::new(Duration::from_secs(30));

    let issue_id = IssueId(100);

    // Before cooldown: should not skip
    assert!(!queue.should_skip(issue_id));

    // Add to cooldown for 3 cycles (3 * 30s = 90s from now)
    queue.cooldown(issue_id, "API returned 500".to_string(), 3);

    // After cooldown: should skip
    assert!(
        queue.should_skip(issue_id),
        "Issue in active cooldown should be skipped"
    );
}

#[tokio::test]
async fn test_expired_entry_not_skipped() {
    let queue = CooldownQueue::new(Duration::from_secs(30));

    let issue_id = IssueId(200);

    // Set cooldown to a time in the past (already expired)
    let past = Utc::now() - TimeDelta::seconds(60);
    queue.cooldown_until(issue_id, "Old failure".to_string(), past);

    // Expired cooldown should not cause skipping
    assert!(
        !queue.should_skip(issue_id),
        "Expired cooldown entry should not cause skipping"
    );
}

#[tokio::test]
async fn test_cleanup_removes_expired() {
    let queue = CooldownQueue::new(Duration::from_secs(30));

    let issue_a = IssueId(301);
    let issue_b = IssueId(302);
    let issue_c = IssueId(303);

    // issue_a: expired (past)
    let past = Utc::now() - TimeDelta::seconds(120);
    queue.cooldown_until(issue_a, "Expired A".to_string(), past);

    // issue_b: still active (future)
    let future = Utc::now() + TimeDelta::seconds(300);
    queue.cooldown_until(issue_b, "Active B".to_string(), future);

    // issue_c: expired (past)
    let past2 = Utc::now() - TimeDelta::seconds(10);
    queue.cooldown_until(issue_c, "Expired C".to_string(), past2);

    assert_eq!(queue.len(), 3);

    // Cleanup should remove expired entries
    queue.cleanup_expired();

    assert_eq!(
        queue.len(),
        1,
        "Only the active entry should remain after cleanup"
    );

    // issue_b should still be in cooldown
    assert!(queue.should_skip(issue_b));

    // issue_a and issue_c should no longer be tracked
    assert!(!queue.should_skip(issue_a));
    assert!(!queue.should_skip(issue_c));
}

#[tokio::test]
async fn test_cooldown_overwrites_existing_entry() {
    let queue = CooldownQueue::new(Duration::from_secs(30));

    let issue_id = IssueId(400);

    // First cooldown: 1 cycle (30s)
    queue.cooldown(issue_id, "First failure".to_string(), 1);
    assert!(queue.should_skip(issue_id));

    // Second cooldown: 5 cycles (150s) — should overwrite
    queue.cooldown(issue_id, "Second failure".to_string(), 5);
    assert!(queue.should_skip(issue_id));

    // Only one entry should exist
    assert_eq!(queue.len(), 1);
}

#[tokio::test]
async fn test_multiple_issues_independent() {
    let queue = CooldownQueue::new(Duration::from_secs(30));

    let issue_a = IssueId(501);
    let issue_b = IssueId(502);
    let issue_c = IssueId(503);

    // Only cooldown issue_a and issue_b
    queue.cooldown(issue_a, "Error A".to_string(), 3);
    queue.cooldown(issue_b, "Error B".to_string(), 2);

    assert!(queue.should_skip(issue_a));
    assert!(queue.should_skip(issue_b));
    assert!(
        !queue.should_skip(issue_c),
        "Issue C was never cooled down"
    );
}

#[tokio::test]
async fn test_zero_cycles_cooldown() {
    let queue = CooldownQueue::new(Duration::from_secs(30));

    let issue_id = IssueId(600);

    // 0 cycles means 0 seconds cooldown — effectively immediate expiry
    queue.cooldown(issue_id, "Zero cooldown".to_string(), 0);

    // With 0 duration, skip_until is essentially "now", so should_skip
    // may or may not return true depending on timing. After a tiny sleep
    // it should definitely be false.
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(
        !queue.should_skip(issue_id),
        "Zero-cycle cooldown should expire immediately"
    );
}

#[tokio::test]
async fn test_cleanup_on_empty_queue() {
    let queue = CooldownQueue::new(Duration::from_secs(30));

    // Cleanup on empty queue should be a no-op
    queue.cleanup_expired();
    assert_eq!(queue.len(), 0);
}

#[tokio::test]
async fn test_concurrent_access() {
    let queue = Arc::new(CooldownQueue::new(Duration::from_secs(30)));

    let mut handles = Vec::new();

    // Spawn multiple tasks that concurrently add and check cooldowns
    for i in 0..10 {
        let q = Arc::clone(&queue);
        let handle = tokio::spawn(async move {
            let issue_id = IssueId(700 + i);
            q.cooldown(issue_id, format!("Concurrent error {}", i), 3);
            assert!(q.should_skip(issue_id));
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(queue.len(), 10);
}
