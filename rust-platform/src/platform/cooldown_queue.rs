//! Issue-level cooldown queue for error isolation.
//!
//! When processing a specific issue fails, it enters the cooldown queue and is
//! skipped for a configurable number of polling cycles. This prevents a single
//! "poison pill" issue from consuming retry budget or blocking other issues.
//!
//! Depends on: `crate::platform::IssueId` (implemented by another agent in src/platform/issue.rs)

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tokio_util::sync::CancellationToken;

// NOTE: IssueId is defined in crate::platform::issue (created by another agent).
// We import it via the platform module re-export.
use crate::platform::IssueId;

/// Internal entry tracking why and until when an issue is cooled down.
#[derive(Debug, Clone)]
struct CooldownEntry {
    /// Human-readable reason for the cooldown (for logging/debugging).
    #[allow(dead_code)]
    reason: String,
    /// The issue will be skipped until this timestamp.
    skip_until: DateTime<Utc>,
}

/// A concurrent-safe queue that tracks issues in cooldown state.
///
/// Uses `DashMap` for lock-free concurrent reads and writes. The queue is
/// ephemeral — it resets on process restart, which is acceptable because
/// cooldown is an optimization, not a correctness guarantee.
#[derive(Debug)]
pub struct CooldownQueue {
    entries: Arc<DashMap<IssueId, CooldownEntry>>,
    polling_interval: Duration,
}

impl CooldownQueue {
    /// Creates a new cooldown queue.
    ///
    /// `polling_interval` is used to calculate cooldown duration:
    /// `cooldown_duration = polling_interval * cycles`.
    pub fn new(polling_interval: Duration) -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
            polling_interval,
        }
    }

    /// Returns `true` if the given issue is currently in cooldown and should be skipped.
    ///
    /// This is a concurrent-safe read operation with no locking overhead.
    pub fn should_skip(&self, issue_id: IssueId) -> bool {
        self.entries
            .get(&issue_id)
            .map(|entry| Utc::now() < entry.skip_until)
            .unwrap_or(false)
    }

    /// Places an issue into cooldown for the specified number of polling cycles.
    ///
    /// If the issue is already in cooldown, the entry is overwritten with the new
    /// duration (extending or shortening the cooldown).
    ///
    /// # Arguments
    ///
    /// * `issue_id` — The issue to cool down.
    /// * `reason` — Human-readable explanation (logged at debug level).
    /// * `cycles` — Number of polling intervals to skip.
    pub fn cooldown(&self, issue_id: IssueId, reason: String, cycles: u32) {
        let cooldown_duration = self.polling_interval.saturating_mul(cycles);
        let skip_until =
            Utc::now() + chrono::TimeDelta::seconds(cooldown_duration.as_secs() as i64);

        tracing::debug!(
            issue_id = %issue_id,
            reason = %reason,
            cycles,
            skip_until = %skip_until,
            "Issue entering cooldown"
        );

        self.entries.insert(
            issue_id,
            CooldownEntry {
                reason,
                skip_until,
            },
        );
    }

    /// Removes all expired entries from the queue.
    ///
    /// Called periodically by the background cleanup task, but can also be
    /// invoked manually (e.g., in tests).
    pub fn cleanup_expired(&self) {
        let now = Utc::now();
        let before = self.entries.len();
        self.entries.retain(|_, entry| now < entry.skip_until);
        let removed = before - self.entries.len();
        if removed > 0 {
            tracing::debug!(removed, remaining = self.entries.len(), "Cleaned up expired cooldown entries");
        }
    }

    /// Spawns a background tokio task that periodically cleans up expired entries.
    ///
    /// The task runs every 60 seconds and respects the provided `CancellationToken`
    /// for graceful shutdown.
    ///
    /// # Arguments
    ///
    /// * `cancel` — When cancelled, the cleanup task exits its loop and terminates.
    pub fn spawn_cleanup_task(&self, cancel: CancellationToken) {
        let entries = Arc::clone(&self.entries);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::debug!("Cooldown cleanup task shutting down");
                        break;
                    }
                    _ = interval.tick() => {
                        let now = Utc::now();
                        entries.retain(|_, entry| now < entry.skip_until);
                    }
                }
            }
        });
    }

    /// Returns the number of issues currently in the cooldown queue (including expired).
    ///
    /// Primarily useful for testing and metrics.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the cooldown queue is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_queue(interval_secs: u64) -> CooldownQueue {
        CooldownQueue::new(Duration::from_secs(interval_secs))
    }

    #[test]
    fn test_should_skip_returns_false_for_unknown_issue() {
        let queue = make_queue(30);
        assert!(!queue.should_skip(IssueId(999)));
    }

    #[test]
    fn test_cooldown_then_skip() {
        let queue = make_queue(30);
        queue.cooldown(IssueId(1), "test error".into(), 3);
        assert!(queue.should_skip(IssueId(1)));
    }

    #[test]
    fn test_expired_entry_not_skipped() {
        let queue = CooldownQueue {
            entries: Arc::new(DashMap::new()),
            polling_interval: Duration::from_secs(1),
        };
        // Insert an entry that's already expired
        queue.entries.insert(
            IssueId(42),
            CooldownEntry {
                reason: "old error".into(),
                skip_until: Utc::now() - chrono::TimeDelta::seconds(10),
            },
        );
        assert!(!queue.should_skip(IssueId(42)));
    }

    #[test]
    fn test_cleanup_removes_expired() {
        let queue = CooldownQueue {
            entries: Arc::new(DashMap::new()),
            polling_interval: Duration::from_secs(1),
        };
        // One expired, one still active
        queue.entries.insert(
            IssueId(1),
            CooldownEntry {
                reason: "expired".into(),
                skip_until: Utc::now() - chrono::TimeDelta::seconds(10),
            },
        );
        queue.entries.insert(
            IssueId(2),
            CooldownEntry {
                reason: "active".into(),
                skip_until: Utc::now() + chrono::TimeDelta::seconds(300),
            },
        );

        assert_eq!(queue.len(), 2);
        queue.cleanup_expired();
        assert_eq!(queue.len(), 1);
        assert!(!queue.should_skip(IssueId(1)));
        assert!(queue.should_skip(IssueId(2)));
    }

    #[test]
    fn test_cooldown_overwrites_existing() {
        let queue = make_queue(30);
        queue.cooldown(IssueId(1), "first".into(), 1);
        queue.cooldown(IssueId(1), "second".into(), 10);
        // Should still be in cooldown (overwritten with longer duration)
        assert!(queue.should_skip(IssueId(1)));
        assert_eq!(queue.len(), 1);
    }

    #[tokio::test]
    async fn test_spawn_cleanup_task_respects_cancellation() {
        let queue = make_queue(30);
        queue.cooldown(IssueId(1), "test".into(), 0); // immediate expiry (0 cycles)

        let cancel = CancellationToken::new();
        queue.spawn_cleanup_task(cancel.clone());

        // Give the task a moment to run at least one tick
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel.cancel();
        // Task should exit gracefully — no panic
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
